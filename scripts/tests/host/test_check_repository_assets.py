#!/usr/bin/env python3
"""Behavior fixtures for scripts/ci/check_repository_assets.py.

Builds temporary Git repositories (no git-lfs, no network) and asserts the
check's findings. Raw index blobs are crafted with ``hash-object
--no-filters`` plus ``update-index --cacheinfo`` to exercise staged paths.
"""

from __future__ import annotations

import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

CHECK_SCRIPT = Path(__file__).resolve().parents[2] / "ci" / "check_repository_assets.py"


def load_check_module():
    spec = importlib.util.spec_from_file_location("check_repository_assets", CHECK_SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


CHECK = load_check_module()

LFS_ATTRS = "*.bin filter=lfs diff=lfs merge=lfs -text\n"
OID_HEX = "a" * 64


def pointer_text(size: int = 123) -> bytes:
    return (
        b"version https://git-lfs.github.com/spec/v1\n"
        b"oid sha256:" + OID_HEX.encode() + b"\n"
        b"size " + str(size).encode() + b"\n"
    )


@unittest.skipUnless(shutil.which("git"), "git is required for repository fixtures")
class CheckRepositoryAssetsTest(unittest.TestCase):
    def setUp(self) -> None:
        environment = dict(os.environ)
        for key in ("GIT_INDEX_FILE", "GIT_DIR", "GIT_WORK_TREE"):
            environment.pop(key, None)
        environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        context = mock.patch.dict(os.environ, environment, clear=True)
        context.start()
        self.addCleanup(context.stop)

    def make_repo(self) -> Path:
        holder = tempfile.TemporaryDirectory()
        self.addCleanup(holder.cleanup)
        root = Path(holder.name)
        self.git(root, "init", "-q")
        self.git(root, "config", "user.email", "test@example.com")
        self.git(root, "config", "user.name", "test")
        self.git(root, "config", "commit.gpgsign", "false")
        return root

    def git(self, root: Path, *args: str, stdin: bytes | None = None) -> bytes:
        result = subprocess.run(
            ["git", "-C", str(root), *args],
            input=stdin,
            capture_output=True,
            check=True,
        )
        return result.stdout

    def write(self, root: Path, name: str, data: bytes) -> Path:
        target = root / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        return target

    def stage_blob(self, root: Path, name: str, data: bytes, mode: str = "100644") -> None:
        oid = self.git(root, "hash-object", "--no-filters", "-w", "--stdin", stdin=data)
        self.git(root, "update-index", "--add", "--cacheinfo", f"{mode},{oid.decode().strip()},{name}")

    def run_check(self, root: Path, *args: str) -> subprocess.CompletedProcess[bytes]:
        return subprocess.run(
            [sys.executable, str(CHECK_SCRIPT), "--root", str(root), *args],
            capture_output=True,
            check=False,
        )

    def rules(self, root: Path, *args: str) -> tuple[subprocess.CompletedProcess[bytes], list[str]]:
        result = self.run_check(root, *args)
        lines = result.stdout.decode().splitlines()
        return result, [line.split(" ", 1)[0] for line in lines if line and not line.startswith("checked")]

    def test_generated_paths_rejected(self) -> None:
        root = self.make_repo()
        for directory in ("logs", "target", "out", "__pycache__", ".scratch", ".embuild"):
            target = self.write(root, f"{directory}/note.txt", b"generated\n")
            self.git(root, "add", str(target.relative_to(root)))
        result, rules = self.rules(root)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(rules.count("generated-path"), 6)

    def test_generated_suffixes_rejected(self) -> None:
        root = self.make_repo()
        for index, suffix in enumerate((".log", ".LOG", ".elf", ".o", ".a", ".rlib", ".rmeta", ".profraw", ".lcov")):
            target = self.write(root, f"build/output{index}{suffix}", b"data\n")
            self.git(root, "add", str(target.relative_to(root)))
        result, rules = self.rules(root)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(rules.count("generated-suffix"), 9)

    def test_small_text_sources_preserved(self) -> None:
        root = self.make_repo()
        self.write(root, "src/main.rs", b"fn main() {}\n")
        self.write(root, "assets/icon.svg", b"<svg></svg>\n")
        self.write(root, "assets/sounds/tune.rtttl", b"tune:d=4,o=5,b=100:c\n")
        self.write(root, "Cargo.lock", b"version = 3\n")
        self.write(root, "fixtures/small.txt", b"fixture\n")
        self.git(root, "add", "-A")
        result = self.run_check(root)
        self.assertEqual(result.returncode, 0, result.stdout.decode())
        self.assertIn(b"0 violations", result.stdout)

    def test_raw_large_text_rejected_in_both_modes(self) -> None:
        root = self.make_repo()
        marker = b"distinctive-content-marker-8491"
        self.write(root, "data/big.txt", marker + b"x" * (1024 * 1024 + 1))
        self.git(root, "add", "data/big.txt")
        result, rules = self.rules(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn("oversize-blob", rules)
        self.assertNotIn(marker, result.stdout)
        result, rules = self.rules(root, "--staged")
        self.assertEqual(result.returncode, 1)
        self.assertIn("oversize-blob", rules)
        self.assertNotIn(marker, result.stdout)

    def test_raw_binary_missing_lfs_rejected_in_both_modes(self) -> None:
        root = self.make_repo()
        self.write(root, "data/raw.bin", b"\0binary\xffdata")
        self.git(root, "add", "data/raw.bin")
        result, rules = self.rules(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn("binary-without-lfs", rules)
        result, rules = self.rules(root, "--staged")
        self.assertEqual(result.returncode, 1)
        self.assertIn("binary-without-lfs", rules)

    def test_partial_lfs_attributes_still_rejected(self) -> None:
        root = self.make_repo()
        self.write(root, ".gitattributes", b"*.bin filter=lfs\n")
        self.write(root, "data/raw.bin", b"\0binary\xffdata")
        self.git(root, "add", "-A")
        result, rules = self.rules(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn("binary-without-lfs", rules)

    def test_registered_hydrated_binary_passes(self) -> None:
        root = self.make_repo()
        self.write(root, ".gitattributes", LFS_ATTRS.encode())
        self.write(root, "data/asset.bin", b"\0hydrated-binary-content")
        result = self.run_check(root)
        self.assertEqual(result.returncode, 0, result.stdout.decode())

    def test_unresolved_pointer_in_worktree_rejected(self) -> None:
        root = self.make_repo()
        self.write(root, ".gitattributes", LFS_ATTRS.encode())
        self.write(root, "data/asset.bin", pointer_text())
        result, rules = self.rules(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn("unresolved-lfs-pointer", rules)

    def test_raw_indexed_lfs_content_rejected(self) -> None:
        root = self.make_repo()
        self.write(root, ".gitattributes", LFS_ATTRS.encode())
        self.git(root, "add", ".gitattributes")
        self.stage_blob(root, "data/asset.bin", b"\0raw-indexed-bytes")
        result, rules = self.rules(root, "--staged")
        self.assertEqual(result.returncode, 1)
        self.assertIn("raw-lfs-content", rules)

    def test_correct_pointer_in_index_passes(self) -> None:
        root = self.make_repo()
        self.write(root, ".gitattributes", LFS_ATTRS.encode())
        self.git(root, "add", ".gitattributes")
        self.stage_blob(root, "data/asset.bin", pointer_text())
        result = self.run_check(root, "--staged")
        self.assertEqual(result.returncode, 0, result.stdout.decode())

    def test_cached_attribute_semantics(self) -> None:
        root = self.make_repo()
        self.write(root, ".gitattributes", LFS_ATTRS.encode())
        self.write(root, "data/asset.bin", b"\0hydrated-binary-content")
        self.git(root, "add", "data/asset.bin")
        result = self.run_check(root)
        self.assertEqual(result.returncode, 0, result.stdout.decode())
        result, rules = self.rules(root, "--staged")
        self.assertEqual(result.returncode, 1)
        self.assertIn("binary-without-lfs", rules)

    def test_noncanonical_staged_pointers_rejected(self) -> None:
        root = self.make_repo()
        self.write(root, ".gitattributes", LFS_ATTRS.encode())
        self.git(root, "add", ".gitattributes")
        variants = (
            pointer_text().rstrip(b"\n"),
            pointer_text().replace(b"size 123", b"size 0123"),
            pointer_text().replace(b"\n", b"\r\n"),
        )
        for index, data in enumerate(variants):
            self.stage_blob(root, f"data/noncanonical{index}.bin", data)
        result, rules = self.rules(root, "--staged")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(rules.count("raw-lfs-content"), len(variants))

    def test_pointer_parser(self) -> None:
        self.assertEqual(CHECK.parse_lfs_pointer(pointer_text(7)), (OID_HEX, 7))
        self.assertIsNone(CHECK.parse_lfs_pointer(b"version https://example.com/spec/v1\noid sha256:" + OID_HEX.encode() + b"\nsize 1\n"))
        self.assertIsNone(CHECK.parse_lfs_pointer(b"version https://git-lfs.github.com/spec/v1\noid sha256:" + b"b" * 63 + b"\nsize 1\n"))
        self.assertIsNone(CHECK.parse_lfs_pointer(b"version https://git-lfs.github.com/spec/v1\noid sha256:" + OID_HEX.upper().encode() + b"\nsize 1\n"))
        self.assertIsNone(CHECK.parse_lfs_pointer(b"version https://git-lfs.github.com/spec/v1\noid sha256:" + OID_HEX.encode() + b"\nsize many\n"))
        self.assertIsNone(CHECK.parse_lfs_pointer(pointer_text() + b"ext foo bar\n"))
        self.assertIsNone(CHECK.parse_lfs_pointer(pointer_text().rstrip(b"\n")))
        self.assertIsNone(CHECK.parse_lfs_pointer(pointer_text().replace(b"size 123", b"size 0123")))
        self.assertIsNone(CHECK.parse_lfs_pointer(b"\0binary"))
        self.assertIsNone(CHECK.parse_lfs_pointer(b""))


if __name__ == "__main__":
    unittest.main()
