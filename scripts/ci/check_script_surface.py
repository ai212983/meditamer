#!/usr/bin/env python3
"""Ratchet the scripts/ surface against scripts/surface.json.

Checks, all blocking:
  1. Exact tracked-path parity: every tracked scripts/**/*.{sh,py} has a
     surface.json entry, and every surface.json entry names a tracked file
     (catches both missing metadata and stale/deleted entries).
  2. No duplicate `path` entries; every entry has a non-empty role (public
     or internal), owner, caller, and reason.
  3. Public executable paths: the live count of role="public" entries must
     equal baselines.public_executable_paths.count exactly (not just fit
     under .ceiling) -- lowering the count on a deletion is mandatory, so
     capacity can't be silently reused by a later unrelated addition. The
     count must also not exceed .ceiling.
  4. Documented leaf commands: the live count of hostctl CLI leaf commands,
     read from tools/hostctl/src/main.rs's `Commands` enum plus each
     container variant's nested subcommand enum (see
     CONTAINER_SUBCOMMAND_ENUMS below -- currently `Test`/`TestSubcommand`
     and `Artifacts`/`ArtifactsSubcommand`), must equal
     baselines.documented_leaf_commands.count exactly, and that baseline must
     carry a non-empty change_log.

Run on script changes, scripts/surface.json changes, and inventory/Markdown
-only changes (this file's own drift is exactly what it exists to catch).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

SCRIPT_SUFFIXES = (".sh", ".py")
# scripts/surface.json and scripts/host-suites.tsv are surface metadata, not
# scripts themselves, and are not entered in surface.json.
NON_SCRIPT_BASENAMES = {"surface.json", "host-suites.tsv"}


def discover_repo_root() -> Path:
    """Default Git-root discovery, matching check_orphan_modules.py (C-401)."""
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        print("check_script_surface.py: must run inside a git work tree", file=sys.stderr)
        raise SystemExit(2)
    return Path(result.stdout.strip())


def tracked_scripts(repo_root: Path) -> set[str]:
    result = subprocess.run(
        ["git", "-C", str(repo_root), "ls-files", "-z", "--", "scripts"],
        check=True,
        capture_output=True,
    )
    paths: set[str] = set()
    for raw in result.stdout.split(b"\0"):
        if not raw:
            continue
        rel = raw.decode("utf-8", errors="surrogateescape")
        path = Path(rel)
        if path.suffix not in SCRIPT_SUFFIXES:
            continue
        if path.name in NON_SCRIPT_BASENAMES:
            continue
        paths.add(rel)
    return paths


# `Commands` variants that are containers dispatching to their own nested
# subcommand enum, rather than leaf commands themselves. Each key's variant
# name must match its value's enum name in tools/hostctl/src/main.rs.
CONTAINER_SUBCOMMAND_ENUMS = {
    "Test": "TestSubcommand",
    "Artifacts": "ArtifactsSubcommand",
}


def count_hostctl_leaf_commands(repo_root: Path) -> int | None:
    main_rs = repo_root / "tools" / "hostctl" / "src" / "main.rs"
    if not main_rs.is_file():
        return None
    text = main_rs.read_text(encoding="utf-8")

    def enum_variant_names(enum_name: str) -> list[str] | None:
        match = re.search(rf"enum {enum_name} \{{(.*?)\n\}}", text, re.DOTALL)
        if not match:
            return None
        body = match.group(1)
        # One `Name(Args)` or bare `Name` variant per non-blank, non-comment line.
        names = []
        for line in body.splitlines():
            line = line.strip()
            if not line or line.startswith("//"):
                continue
            name_match = re.match(r"([A-Za-z_][A-Za-z0-9_]*)", line)
            if not name_match:
                return None
            names.append(name_match.group(1))
        return names

    command_names = enum_variant_names("Commands")
    if command_names is None:
        return None

    total = 0
    for name in command_names:
        nested_enum = CONTAINER_SUBCOMMAND_ENUMS.get(name)
        if nested_enum is None:
            total += 1
            continue
        nested_names = enum_variant_names(nested_enum)
        if nested_names is None:
            return None
        total += len(nested_names)
    return total


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=None,
        help="repository root (default: `git rev-parse --show-toplevel`)",
    )
    parser.add_argument(
        "--surface-path",
        type=Path,
        default=None,
        help="path to surface.json (default: <repo-root>/scripts/surface.json)",
    )
    args = parser.parse_args()

    repo_root = (args.repo_root or discover_repo_root()).resolve()
    surface_path = args.surface_path or (repo_root / "scripts" / "surface.json")

    if not surface_path.is_file():
        print(f"check_script_surface.py: missing {surface_path}", file=sys.stderr)
        return 2

    try:
        surface = json.loads(surface_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        print(f"check_script_surface.py: invalid JSON in {surface_path}: {error}", file=sys.stderr)
        return 2

    failures: list[str] = []

    entries = surface.get("scripts", [])
    entry_paths = [entry.get("path", "") for entry in entries]

    # 1. Exact tracked-path parity.
    tracked = tracked_scripts(repo_root)
    surfaced = set(entry_paths)
    missing_from_surface = sorted(tracked - surfaced)
    stale_in_surface = sorted(surfaced - tracked)
    for path in missing_from_surface:
        failures.append(f"tracked script has no surface.json entry: {path}")
    for path in stale_in_surface:
        failures.append(f"surface.json entry names a non-tracked/non-script path: {path}")

    # 2. Duplicates and required fields.
    seen: set[str] = set()
    for entry in entries:
        path = entry.get("path", "")
        if path in seen:
            failures.append(f"duplicate surface.json entry: {path}")
        seen.add(path)
        role = entry.get("role")
        if role not in ("public", "internal"):
            failures.append(f"{path}: role must be 'public' or 'internal', got {role!r}")
        for field in ("owner", "caller", "reason"):
            if not entry.get(field, "").strip():
                failures.append(f"{path}: missing or empty '{field}'")

    # 3. Public executable path ratchet.
    baselines = surface.get("baselines", {})
    public_baseline = baselines.get("public_executable_paths", {})
    public_count_expected = public_baseline.get("count")
    public_ceiling = public_baseline.get("ceiling")
    public_count_actual = sum(1 for entry in entries if entry.get("role") == "public")
    if public_count_expected is None:
        failures.append("surface.json missing baselines.public_executable_paths.count")
    elif public_count_actual != public_count_expected:
        failures.append(
            "public executable path count drifted from its ratcheted baseline: "
            f"live={public_count_actual} baseline={public_count_expected}. "
            "Update baselines.public_executable_paths.count in scripts/surface.json "
            "to the live count (this is required on every add or removal of a "
            "public-role script, not just when it grows)."
        )
    if public_ceiling is not None and public_count_actual > public_ceiling:
        failures.append(
            f"public executable path count {public_count_actual} exceeds the plan's "
            f"closeout ceiling of {public_ceiling}"
        )

    # 4. Documented leaf command ratchet.
    leaf_baseline = baselines.get("documented_leaf_commands", {})
    leaf_count_expected = leaf_baseline.get("count")
    leaf_change_log = leaf_baseline.get("change_log")
    if leaf_count_expected is None:
        failures.append("surface.json missing baselines.documented_leaf_commands.count")
    if not leaf_change_log:
        failures.append("surface.json missing baselines.documented_leaf_commands.change_log")
    leaf_count_actual = count_hostctl_leaf_commands(repo_root)
    if leaf_count_actual is None:
        failures.append(
            "could not introspect hostctl leaf commands from tools/hostctl/src/main.rs "
            "(Commands/TestSubcommand enum shape changed -- update this checker's parser "
            "and record the new baseline with a change_log entry)"
        )
    elif leaf_count_expected is not None and leaf_count_actual != leaf_count_expected:
        failures.append(
            "hostctl CLI leaf command count drifted from its recorded baseline: "
            f"live={leaf_count_actual} baseline={leaf_count_expected}. "
            "Update baselines.documented_leaf_commands.count in scripts/surface.json "
            "and append a change_log entry describing what changed."
        )

    total_scripts = len(tracked)
    print(
        f"script-surface: {total_scripts} tracked script(s), "
        f"{len(entries)} surface.json entr{'y' if len(entries) == 1 else 'ies'}, "
        f"public={public_count_actual}"
        + (f"/{public_count_expected}" if public_count_expected is not None else "")
        + f", hostctl leaf commands={leaf_count_actual}"
        + (f"/{leaf_count_expected}" if leaf_count_expected is not None else "")
    )

    if failures:
        print(f"script-surface: {len(failures)} violation(s):", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("script-surface: clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
