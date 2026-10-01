# Development setup

Firmware uses the `esp` Rust toolchain and target-specific Xtensa compilers;
host tools use `stable`. Build wrappers load `~/export-esp.sh` when available.
Prepare that toolchain environment before the [first build](build-and-flash.md).
Inkplate targets ESP32; Medinote/Waveshare targets ESP32-S3.

## Install host tooling and hooks

```bash
brew install git-lfs lefthook lychee jq
git lfs install --local --skip-repo
git lfs pull
cargo install --locked rust-code-analysis-cli --version 0.0.25
go install github.com/conventionalcommit/commitlint@latest
lefthook install
```

Cargo installations of `lefthook` and `lychee` can replace Homebrew.
[lefthook.yml](../../../lefthook.yml) owns hook commands and path filters.
Pre-commit can format and stage Rust changes; inspect the staged diff afterward.
Commit messages use `type(scope): subject`; the
[commit checker](../../../scripts/ci/check_commit_message.sh) owns allowed scopes.

## Binary assets and repository hygiene

[Git attributes](../../../.gitattributes) route retained binary inputs and
references through Git LFS: BIN, PNG, JPEG, BMP, GIF, WebP, TTF, OTF, WOFF/WOFF2
and PDF, including uppercase extensions. Rust sources, SVG, sound scores and
lockfiles remain ordinary Git text. Run `git lfs pull` after cloning to download
the binary inputs. Lefthook automatically runs `git lfs pre-push` alongside the
repository checks.

[Ignore rules](../../../.gitignore) exclude nested logs, build/output directories,
compiler artifacts, coverage and Python caches. Keep qualification summaries in
maintained documentation and captures in ignored `logs/` directories. Selected
source assets and reproducible test/specimen inputs remain versioned through LFS.

When adopting or changing LFS rules, normalize existing tracked assets before
committing. This updates the next commit without rewriting earlier history:

```bash
git add .gitattributes .gitignore
git add --renormalize -- assets platform targets
git add --all
scripts/ci/check_repository_assets.py --staged
```

The [asset guard](../../../scripts/ci/check_repository_assets.py) rejects generated
artifacts, ordinary Git blobs above 1 MiB, unregistered binary data and staged
LFS files that contain payloads instead of canonical pointers. Its default mode
checks the worktree, including new files, and rejects unhydrated LFS pointers.

## Software checks

All checks run on the development machine; GitHub Actions workflows are removed.
Run the complete software baseline before publishing changes. Hooks run selected
checks, and individual lanes are available for focused work:

```bash
scripts/ci/check_software_baseline.sh all
scripts/ci/check_software_baseline.sh source
scripts/ci/check_software_baseline.sh host
scripts/ci/check_software_baseline.sh firmware
scripts/ci/check_software_baseline.sh static
scripts/ci/check_software_baseline.sh quality
```

For a focused host suite or workflow change:

```bash
scripts/host-test.sh test hostctl
scripts/host-test.sh test app-state
```

The [suite registry](../../../scripts/host-suites.tsv) owns test/lint/coverage
membership. Firmware checks build and lint explicit compositions; hardware
qualification is separate. A software pass does not prove a device was exercised.

For documentation:

```bash
scripts/ci/check_markdown_links.sh --all
scripts/ci/check_markdown_loc.sh
```

`--all` selects tracked live Markdown. Pass new files explicitly before staging.
Link checks are offline by default; use `MARKDOWN_LINKS_ONLINE=1` to include remote
links. Vendor and archive documents are excluded.

## Source review

```bash
scripts/ci/lint_code_analysis.sh
RCA_ENFORCE=1 RCA_RATCHET=1 scripts/ci/lint_code_analysis.sh
scripts/ci/check_stack_risk.sh
```

Production SLOC/complexity ratchets, generated-only `include!`,
and Cargo-target reachability are enforced by the quality lane. Split at coherent
responsibility or lifetime boundaries; do not shard code to reduce line counts.
Refresh `config/rca-baseline.json` only for an intentional refactor named by an
active plan, using `RCA_UPDATE_BASELINE=1 scripts/ci/lint_code_analysis.sh`.

## Coverage

```bash
rustup component add llvm-tools-preview --toolchain stable
cargo install --locked cargo-llvm-cov
scripts/ci/coverage_host.sh
```

Coverage writes `logs/coverage/host_coverage.lcov`. Optional controls are
`HOST_COVERAGE_OUTPUT_DIR`, `HOST_COVERAGE_MIN_LINE` (default `0`), and
`RUSTUP_TOOLCHAIN` (default `stable`).

## Evidence cleanup

```bash
scripts/hostctl.sh artifacts inventory
scripts/hostctl.sh artifacts prune
```

Pruning defaults to dry-run payload thinning. Review candidates before adding
`--apply`. For whole-run and standalone-log expiry, preview with `--runs` and
review its larger candidate list before using `--apply --runs`.
[Hostctl artifacts](../../references/hostctl.md#artifact-retention) owns retention
windows and `.retain.json`. Retain candidate evidence needed for qualification.
