#!/usr/bin/env python3
"""Report tracked first-party Rust files unreachable from any Cargo target."""

from __future__ import annotations

import argparse
import collections
import re
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path


FIRST_PARTY_PREFIXES = ("src/", "packages/", "tools/", "test-support/")
SOURCE_AREAS = ("src", "tests", "examples", "benches")
NON_SOURCE_DIR_NAMES = {"fixtures", "snapshots", "testdata"}
ATTRIBUTE_BLOCK = r"(?:\s*#\s*\[[^\]]*\]\s*)*"
MOD_RE = re.compile(
    rf"(?P<attrs>{ATTRIBUTE_BLOCK})"
    r"(?:pub(?:\s*\([^)]*\))?\s+)?(?:unsafe\s+)?"
    r"mod\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*;",
    re.MULTILINE,
)
PATH_RE = re.compile(r'path\s*=\s*"(?P<path>[^"\r\n]+)"')
INCLUDE_RE = re.compile(r'include!\s*\(\s*"(?P<path>[^"\r\n]+)"\s*\)')


@dataclass(frozen=True)
class TargetRoot:
    label: str
    path: Path


@dataclass(frozen=True)
class ModuleState:
    path: Path
    module_dir: Path
    target: str


def git_paths(repo_root: Path, source_root: Path, *pathspecs: str) -> list[Path]:
    command = ["git", "-C", str(repo_root), "ls-files", "-z"]
    if pathspecs:
        command.extend(("--", *pathspecs))
    result = subprocess.run(command, check=True, capture_output=True)
    return [
        source_root / raw.decode("utf-8", errors="surrogateescape")
        for raw in result.stdout.split(b"\0")
        if raw
    ]


def is_first_party(path: Path, repo_root: Path) -> bool:
    relative = path.relative_to(repo_root).as_posix()
    return relative in {"Cargo.toml", "build.rs"} or relative.startswith(FIRST_PARTY_PREFIXES)


def is_rust_source_candidate(path: Path, area_root: Path) -> bool:
    relative_parts = path.relative_to(area_root).parts
    return not any(part in NON_SOURCE_DIR_NAMES for part in relative_parts[:-1])


def existing_tracked_rust(repo_root: Path, source_root: Path) -> set[Path]:
    return {
        path.resolve()
        for path in git_paths(
            repo_root, source_root, "src", "packages", "tools", "test-support", "build.rs"
        )
        if path.suffix == ".rs" and path.is_file() and is_first_party(path, source_root)
    }


def manifest_paths(repo_root: Path, source_root: Path) -> list[Path]:
    paths = git_paths(
        repo_root, source_root, "Cargo.toml", "packages", "tools", "test-support"
    )
    return sorted(
        path.resolve()
        for path in paths
        if path.name == "Cargo.toml" and path.is_file() and is_first_party(path, source_root)
    )


def target_path(package_dir: Path, raw: str | None, fallback: Path | None) -> Path | None:
    if raw:
        return (package_dir / raw).resolve()
    if fallback is not None and fallback.is_file():
        return fallback.resolve()
    return None


def auto_roots(directory: Path) -> list[Path]:
    if not directory.is_dir():
        return []
    roots = [path.resolve() for path in directory.glob("*.rs") if path.is_file()]
    roots.extend(
        path.resolve() for path in directory.glob("*/main.rs") if path.is_file()
    )
    return sorted(set(roots))


def explicit_roots(
    package_dir: Path,
    entries: list[dict[str, object]],
    default_dir: str,
) -> list[tuple[str, Path]]:
    roots: list[tuple[str, Path]] = []
    for entry in entries:
        name = str(entry.get("name", "unnamed"))
        raw_path = entry.get("path")
        path = target_path(
            package_dir,
            str(raw_path) if isinstance(raw_path, str) else None,
            package_dir / default_dir / f"{name}.rs",
        )
        if path is None:
            directory_main = package_dir / default_dir / name / "main.rs"
            if directory_main.is_file():
                path = directory_main.resolve()
        if path is not None:
            roots.append((name, path))
    return roots


def package_targets(manifest: Path) -> tuple[list[TargetRoot], set[Path]]:
    with manifest.open("rb") as handle:
        data = tomllib.load(handle)
    package = data.get("package")
    if not isinstance(package, dict):
        return [], set()

    package_dir = manifest.parent.resolve()
    package_name = str(package.get("name", package_dir.name))
    roots: list[TargetRoot] = []

    lib = data.get("lib")
    if isinstance(lib, dict):
        raw_path = lib.get("path")
        path = target_path(
            package_dir,
            str(raw_path) if isinstance(raw_path, str) else None,
            package_dir / "src/lib.rs",
        )
        if path is not None:
            roots.append(TargetRoot(f"{package_name}:lib", path))
    elif (package_dir / "src/lib.rs").is_file():
        roots.append(TargetRoot(f"{package_name}:lib", (package_dir / "src/lib.rs").resolve()))

    explicit_bins = data.get("bin", [])
    if isinstance(explicit_bins, list):
        for name, path in explicit_roots(package_dir, explicit_bins, "src/bin"):
            roots.append(TargetRoot(f"{package_name}:bin:{name}", path))

    if package.get("autobins", True):
        default_main = package_dir / "src/main.rs"
        if default_main.is_file():
            roots.append(TargetRoot(f"{package_name}:bin:{package_name}", default_main.resolve()))
        for path in auto_roots(package_dir / "src/bin"):
            roots.append(TargetRoot(f"{package_name}:bin:{path.stem}", path))

    target_kinds = (
        ("test", "tests", "autotests"),
        ("example", "examples", "autoexamples"),
        ("bench", "benches", "autobenches"),
    )
    for table_name, directory, auto_key in target_kinds:
        entries = data.get(table_name, [])
        if isinstance(entries, list):
            for name, path in explicit_roots(package_dir, entries, directory):
                roots.append(TargetRoot(f"{package_name}:{table_name}:{name}", path))
        if package.get(auto_key, True):
            for path in auto_roots(package_dir / directory):
                roots.append(TargetRoot(f"{package_name}:{table_name}:{path.stem}", path))

    build_value = package.get("build")
    build_path: Path | None = None
    if build_value is not False:
        if isinstance(build_value, str):
            build_path = target_path(package_dir, build_value, None)
        elif (package_dir / "build.rs").is_file():
            build_path = (package_dir / "build.rs").resolve()
    if build_path is not None:
        roots.append(TargetRoot(f"{package_name}:build", build_path))

    candidates: set[Path] = set()
    for area in SOURCE_AREAS:
        area_root = package_dir / area
        if area_root.is_dir():
            candidates.update(
                path.resolve()
                for path in area_root.rglob("*.rs")
                if path.is_file() and is_rust_source_candidate(path, area_root)
            )
    candidates.update(root.path for root in roots if root.path.is_file())
    return roots, candidates


def strip_comments(source: str) -> str:
    """Remove Rust comments while preserving strings and line positions."""

    output: list[str] = []
    index = 0
    block_depth = 0
    in_string = False
    escaped = False
    while index < len(source):
        current = source[index]
        following = source[index + 1] if index + 1 < len(source) else ""

        if block_depth:
            if current == "/" and following == "*":
                block_depth += 1
                output.extend((" ", " "))
                index += 2
            elif current == "*" and following == "/":
                block_depth -= 1
                output.extend((" ", " "))
                index += 2
            else:
                output.append("\n" if current == "\n" else " ")
                index += 1
            continue

        if in_string:
            output.append(current)
            if escaped:
                escaped = False
            elif current == "\\":
                escaped = True
            elif current == '"':
                in_string = False
            index += 1
            continue

        if current == '"':
            in_string = True
            output.append(current)
            index += 1
        elif current == "/" and following == "/":
            output.extend((" ", " "))
            index += 2
            while index < len(source) and source[index] != "\n":
                output.append(" ")
                index += 1
        elif current == "/" and following == "*":
            block_depth = 1
            output.extend((" ", " "))
            index += 2
        else:
            output.append(current)
            index += 1
    return "".join(output)


def module_dir_for(path: Path) -> Path:
    if path.name in {"lib.rs", "main.rs", "mod.rs", "build.rs"}:
        return path.parent
    return path.with_suffix("")


def resolve_module(source: Path, module_dir: Path, attrs: str, name: str) -> list[Path]:
    path_match = PATH_RE.search(attrs)
    if path_match:
        candidate = (source.parent / path_match.group("path")).resolve()
        return [candidate] if candidate.is_file() else []

    candidates = [
        (module_dir / f"{name}.rs").resolve(),
        (module_dir / name / "mod.rs").resolve(),
    ]
    return [candidate for candidate in candidates if candidate.is_file()]


def source_edges(source: Path, module_dir: Path) -> list[tuple[Path, Path]]:
    cleaned = strip_comments(source.read_text(encoding="utf-8", errors="replace"))
    edges: list[tuple[Path, Path]] = []
    for match in MOD_RE.finditer(cleaned):
        for child in resolve_module(
            source,
            module_dir,
            match.group("attrs"),
            match.group("name"),
        ):
            edges.append((child, module_dir_for(child)))
    for match in INCLUDE_RE.finditer(cleaned):
        child = (source.parent / match.group("path")).resolve()
        if child.is_file():
            edges.append((child, module_dir))
    return edges


def find_reachable(roots: list[TargetRoot]) -> tuple[set[Path], dict[Path, set[str]]]:
    reachable: set[Path] = set()
    reached_by: dict[Path, set[str]] = collections.defaultdict(set)
    queue = collections.deque(
        ModuleState(root.path, root.path.parent, root.label)
        for root in roots
        if root.path.is_file()
    )
    visited: set[ModuleState] = set()
    while queue:
        state = queue.popleft()
        if state in visited:
            continue
        visited.add(state)
        reachable.add(state.path)
        reached_by[state.path].add(state.target)
        for child, child_module_dir in source_edges(state.path, state.module_dir):
            queue.append(ModuleState(child, child_module_dir, state.target))
    return reachable, reached_by


def discover_repo_root() -> Path:
    """Default Git-root discovery, mirroring the retired shell shim exactly."""
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        print("check_orphan_modules.py: must run inside a git work tree", file=sys.stderr)
        raise SystemExit(2)
    return Path(result.stdout.strip())


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--staged",
        action="store_true",
        help="analyze the complete Git index tree instead of worktree content",
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=None,
        help="repository root (default: `git rev-parse --show-toplevel` from the "
        "current directory; explicit override for fixtures outside a Git checkout)",
    )
    return parser.parse_args()


def check_repo(repo_root: Path, source_root: Path, source_label: str) -> int:
    tracked = existing_tracked_rust(repo_root, source_root)
    roots: list[TargetRoot] = []
    candidates: set[Path] = set()
    manifests = manifest_paths(repo_root, source_root)
    for manifest in manifests:
        package_roots, package_candidates = package_targets(manifest)
        roots.extend(package_roots)
        candidates.update(package_candidates)

    candidates.intersection_update(tracked)
    reachable, _ = find_reachable(roots)
    orphans = sorted(candidates - reachable)

    print(
        "orphan-modules: "
        f"checked {len(candidates)} tracked Rust file(s) from "
        f"{len(roots)} Cargo target root(s) in {len(manifests)} manifest(s) "
        f"({source_label})"
    )
    if not orphans:
        print("orphan-modules: zero unreachable tracked Rust files")
        return 0

    print(f"orphan-modules: unreachable tracked Rust files ({len(orphans)}):", file=sys.stderr)
    for orphan in orphans:
        print(f"  - {orphan.relative_to(source_root).as_posix()}", file=sys.stderr)
    return 1


def main() -> int:
    args = parse_args()
    repo_root = (args.repo_root or discover_repo_root()).resolve()
    if not args.staged:
        return check_repo(repo_root, repo_root, "worktree content")

    with tempfile.TemporaryDirectory(prefix="meditamer-index-") as temp_dir:
        source_root = Path(temp_dir).resolve()
        subprocess.run(
            [
                "git",
                "-C",
                str(repo_root),
                "checkout-index",
                "--all",
                f"--prefix={source_root}{Path('/')}",
            ],
            check=True,
        )
        return check_repo(repo_root, source_root, "Git index content")


if __name__ == "__main__":
    raise SystemExit(main())
