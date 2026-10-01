#!/usr/bin/env python3
"""Reject generated artifacts, oversize blobs, and non-LFS binaries.

Default mode scans existing tracked plus non-ignored untracked worktree
files. ``--staged`` scans all files from the index using batch ``cat-file``.
Findings print one ``rule path detail`` line each with a totals summary and a
nonzero exit status on any violation. File contents are never printed.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

MAX_BLOB_BYTES = 1024 * 1024
HEAD_BYTES = 8192
POINTER_READ_CAP = 65536
DRAIN_CHUNK = 65536

GENERATED_DIRS = frozenset(
    {"logs", "target", "out", "__pycache__", ".scratch", ".embuild"}
)
GENERATED_SUFFIXES = (".log", ".elf", ".o", ".a", ".rlib", ".rmeta", ".profraw", ".lcov")
CHECK_ATTRS = ("filter", "diff", "merge", "text")
LFS_VERSION_LINE = "version https://git-lfs.github.com/spec/v1"
POINTER_RE = re.compile(
    rb"version https://git-lfs.github.com/spec/v1\n"
    rb"oid sha256:([0-9a-f]{64})\nsize (0|[1-9][0-9]{0,19})\n"
)


def run_git(repo: Path, *args: str, stdin: bytes | None = None) -> bytes:
    result = subprocess.run(
        ["git", "-C", str(repo), *args],
        input=stdin,
        capture_output=True,
        check=True,
    )
    return result.stdout


@dataclass
class Finding:
    rule: str
    path: str
    detail: str


def display(path: bytes) -> str:
    return os.fsdecode(path)


def generated_component(path: bytes) -> str | None:
    parts = path.split(b"/")
    for part in parts[:-1]:
        name = os.fsdecode(part)
        if name in GENERATED_DIRS:
            return name
    return None


def generated_suffix(path: bytes) -> str | None:
    name = os.fsdecode(path.split(b"/")[-1]).lower()
    for suffix in GENERATED_SUFFIXES:
        if name.endswith(suffix):
            return suffix
    return None


def is_lfs_matched(attrs: dict[str, str]) -> bool:
    return (
        attrs.get("filter") == "lfs"
        and attrs.get("diff") == "lfs"
        and attrs.get("merge") == "lfs"
        and attrs.get("text") == "unset"
    )


def parse_lfs_pointer(data: bytes) -> tuple[str, int] | None:
    match = POINTER_RE.fullmatch(data)
    if match is None:
        return None
    return match[1].decode("ascii"), int(match[2])


def is_lfs_pointer_head(head: bytes) -> bool:
    try:
        first = head.decode("ascii").split("\n", 1)[0]
    except UnicodeDecodeError:
        return False
    return first == LFS_VERSION_LINE


def worktree_paths(repo: Path) -> list[bytes]:
    paths: list[bytes] = []
    for args in (["ls-files", "-z"], ["ls-files", "--others", "--exclude-standard", "-z"]):
        raw = run_git(repo, *args)
        for entry in raw.split(b"\0"):
            if entry:
                paths.append(entry)
    seen: set[bytes] = set()
    unique = [path for path in paths if path not in seen and not seen.add(path)]
    return [path for path in unique if os.path.isfile(os.path.join(os.fsencode(repo), path))]


def index_entries(repo: Path) -> list[tuple[bytes, bytes]]:
    raw = run_git(repo, "ls-files", "-s", "-z")
    entries: list[tuple[bytes, bytes]] = []
    for record in raw.split(b"\0"):
        if not record:
            continue
        meta, _, path = record.partition(b"\t")
        fields = meta.split(b" ")
        if len(fields) != 3 or fields[0] == b"160000":
            continue
        if fields[2] != b"0":
            raise ValueError(f"unmerged index entry: {display(path)}")
        entries.append((fields[1], path))
    return entries


def batch_attrs(repo: Path, paths: list[bytes], cached: bool) -> dict[bytes, dict[str, str]]:
    if not paths:
        return {}
    command = ["check-attr"]
    if cached:
        command.append("--cached")
    command.extend(["--stdin", "-z", *CHECK_ATTRS])
    raw = run_git(repo, *command, stdin=b"\0".join(paths) + b"\0")
    chunks = raw.split(b"\0")
    attrs: dict[bytes, dict[str, str]] = {}
    for index in range(0, len(chunks) - 1, 3):
        path, name, info = chunks[index : index + 3]
        attrs.setdefault(path, {})[os.fsdecode(name)] = os.fsdecode(info)
    return attrs


def batch_blobs(repo: Path, oids: list[bytes]) -> dict[bytes, tuple[int, bytes, bytes | None]]:
    if not oids:
        return {}
    command = ["git", "-C", str(repo), "cat-file", "--batch"]
    worker = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    assert worker.stdin is not None and worker.stdout is not None
    blobs: dict[bytes, tuple[int, bytes, bytes | None]] = {}
    try:
        for oid in dict.fromkeys(oids):
            worker.stdin.write(oid + b"\n")
            worker.stdin.flush()
            header = worker.stdout.readline().split()
            if len(header) != 3 or header[1] != b"blob":
                raise ValueError(f"unavailable indexed blob: {oid.decode('ascii')}")
            size = int(header[2])
            wanted = min(size, max(HEAD_BYTES, POINTER_READ_CAP))
            data = worker.stdout.read(wanted)
            head = data[:HEAD_BYTES]
            content = data if size <= POINTER_READ_CAP else None
            remaining = size - len(data)
            while remaining > 0:
                chunk = worker.stdout.read(min(DRAIN_CHUNK, remaining))
                if not chunk:
                    raise ValueError("truncated indexed blob")
                remaining -= len(chunk)
            if worker.stdout.read(1) != b"\n":
                raise ValueError("invalid indexed blob delimiter")
            blobs[oid] = (size, head, content)
    finally:
        worker.stdin.close()
        worker.stdout.close()
        returncode = worker.wait()
    if returncode:
        raise subprocess.CalledProcessError(returncode, command)
    return blobs


def evaluate(path: bytes, size: int, head: bytes, attrs: dict[str, str],
             staged: bool, content: bytes | None) -> list[Finding]:
    findings: list[Finding] = []
    name = display(path)
    component = generated_component(path)
    if component is not None:
        findings.append(Finding("generated-path", name, f"component={component}"))
    suffix = generated_suffix(path)
    if suffix is not None:
        findings.append(Finding("generated-suffix", name, f"suffix={suffix}"))
    if is_lfs_matched(attrs):
        if staged:
            pointer = parse_lfs_pointer(content) if content is not None else None
            if pointer is None:
                findings.append(Finding("raw-lfs-content", name, f"{size} bytes, not a canonical pointer"))
        elif is_lfs_pointer_head(head):
            findings.append(Finding("unresolved-lfs-pointer", name, "worktree holds an LFS pointer"))
        return findings
    if size > MAX_BLOB_BYTES:
        findings.append(Finding("oversize-blob", name, f"{size} bytes > {MAX_BLOB_BYTES} bytes"))
    if b"\0" in head:
        findings.append(Finding("binary-without-lfs", name, "NUL byte without LFS attributes"))
    return findings


def scan_worktree(repo: Path) -> tuple[list[Finding], int]:
    paths = worktree_paths(repo)
    attr_map = batch_attrs(repo, paths, cached=False)
    findings: list[Finding] = []
    root = os.fsencode(repo)
    for path in paths:
        full = os.path.join(root, path)
        try:
            with open(full, "rb") as handle:
                head = handle.read(HEAD_BYTES)
            size = os.path.getsize(full)
        except OSError as error:
            findings.append(Finding("unreadable-file", display(path), f"errno={error.errno}"))
            continue
        findings.extend(evaluate(path, size, head, attr_map.get(path, {}), False, None))
    return findings, len(paths)


def scan_staged(repo: Path) -> tuple[list[Finding], int]:
    entries = index_entries(repo)
    paths = [path for _, path in entries]
    attr_map = batch_attrs(repo, paths, cached=True)
    blob_map = batch_blobs(repo, [oid for oid, _ in entries])
    findings: list[Finding] = []
    for oid, path in entries:
        size, head, content = blob_map[oid]
        findings.extend(evaluate(path, size, head, attr_map.get(path, {}), True, content))
    return findings, len(entries)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Reject generated artifacts, oversize blobs, and non-LFS binaries.")
    parser.add_argument("--staged", action="store_true", help="scan all files from the index")
    parser.add_argument("--root", default=None, help="repository root (default: git top level)")
    args = parser.parse_args(argv)
    repo = Path(args.root) if args.root else Path(run_git(Path.cwd(), "rev-parse", "--show-toplevel").decode().strip())
    findings, total = scan_staged(repo) if args.staged else scan_worktree(repo)
    for finding in sorted(findings, key=lambda item: (item.path, item.rule)):
        print(f"{finding.rule} {finding.path} {finding.detail}")
    mode = "staged" if args.staged else "worktree"
    print(f"checked {total} {mode} files, {len(findings)} violations")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
