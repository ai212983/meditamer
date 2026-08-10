# Development Setup

## Runtime Stack

The firmware now targets `esp-hal` (`xtensa-esp32-none-elf`) as the primary and only runtime path.

## Git Hooks

This repo uses [`lefthook`](https://github.com/evilmartians/lefthook) to manage hooks (Husky-like, but language-agnostic).

Install dependencies:

```bash
brew install lefthook lychee jq
```

Or via Cargo:

```bash
cargo install --locked lefthook
cargo install --locked lychee
cargo install --locked rust-code-analysis-cli --version 0.0.25
```

Install conventional commit linter:

```bash
go install github.com/conventionalcommit/commitlint@latest
```

Install host coverage tooling:

```bash
rustup component add llvm-tools-preview --toolchain stable
cargo install --locked cargo-llvm-cov
```

Install hooks:

```bash
scripts/ci/setup_hooks.sh
```

Current pre-commit hook:

- Runs `cargo fmt --all` when staged Rust files match `src/**/*.rs`, `tools/**/*.rs`, or `build.rs`.
- Auto-stages formatter edits (`stage_fixed: true`) so commits include rustfmt output.
- Validates links in staged Markdown files via `scripts/ci/check_markdown_links.sh`.
- Scans staged files for leaked Wi-Fi credentials via `scripts/ci/check_secrets.sh --staged`.
- Uses `lychee` in `--offline` mode by default for reliable local commits.
- Runs the canonical host-lint lane (`-D warnings`) when staged files touch host tools, the SD-card package, or workspace toolchain manifests.

Current commit-msg hook:

- Validates commit messages against Conventional Commits via `scripts/ci/check_commit_message.sh` and `commitlint`.
- Requires a scope in `type(scope): subject` format.
- Enforces allowed scopes: `runtime`, `touch`, `event-engine`, `storage`, `upload`, `wifi`, `telemetry`, `graphics`, `drivers`, `tooling`, `ci`, `docs`.
- Exempts Git-generated/autosquash subjects (`Merge ...`, `Revert ...`, `fixup! ...`, `squash! ...`) from custom scope checks.

Current pre-push hook:

- Runs strict firmware clippy through `scripts/build/build.sh clippy`, which shares the production LVGL native-toolchain setup, when pushed files touch firmware/workspace Rust paths.
- Runs strict code-metrics ratchet via `RCA_ENFORCE=1 RCA_RATCHET=1 scripts/ci/lint_code_analysis.sh` on Rust/workspace changes.

CI includes a dedicated secret-scan workflow (`.github/workflows/secret_scan.yml`) that runs `scripts/ci/check_secrets.sh` on pull requests and pushes to `master`.

Code analysis lint command (report mode by default):

```bash
scripts/ci/lint_code_analysis.sh
```

Strict ratchet mode (used by pre-push and CI):

```bash
RCA_ENFORCE=1 RCA_RATCHET=1 scripts/ci/lint_code_analysis.sh
```

Refresh ratchet baseline after intentional refactors:

```bash
RCA_UPDATE_BASELINE=1 scripts/ci/lint_code_analysis.sh
```

Rust-analyzer baseline lint:

```bash
scripts/ci/lint_rust_analyzer.sh
```

Stack-risk guard (host-side static check for large fixed stack arrays):

```bash
scripts/ci/check_stack_risk.sh
```

Host coverage (line coverage + LCOV artifacts):

```bash
scripts/ci/coverage_host.sh
```

Optional host coverage env vars:

- `HOST_COVERAGE_OUTPUT_DIR` (default `./logs/coverage`)
- `HOST_COVERAGE_MIN_LINE` (default `0`; fail if any crate is below this percent)
- `RUSTUP_TOOLCHAIN` (default `stable`)

SonarQube scan (local server):

```bash
scripts/ci/sonar_scan.sh
```

By default the scan runs `scripts/ci/coverage_host.sh` first and imports
`logs/coverage/host_coverage.lcov` via `sonar.rust.lcov.reportPaths`.

Optional SonarQube env vars:

- `SONAR_TOKEN` (required; auto-loaded from `.env.local` when present)
- `SONAR_HOST_URL` (default `http://localhost:9000`)
- `SONAR_PROJECT_KEY` (default `Meditamer`)
- `SONAR_RUN_HOST_COVERAGE` (`1` default; set `0` to skip pre-scan host coverage)
- `SONAR_POLL_CE` (`1` default; set `0` to skip waiting for CE completion)
- `SONAR_CE_TIMEOUT_SEC` (default `300`)
- `SONAR_CE_POLL_INTERVAL_SEC` (default `2`)
- `SONAR_ENV_FILE` (optional override for env file path; defaults to `.env.local`, falls back to `.env` if missing)

Notes for this workspace:

- The firmware is `no_std`; the optional Wi-Fi, telemetry, and slim diagnostic profiles can still produce analyzer noise outside the active build profile.
- The baseline script intentionally runs with `--disable-build-scripts --disable-proc-macros` for stable, fast CI signal.
- Authoritative correctness gates remain `cargo +esp check -Zbuild-std=core,alloc --target xtensa-esp32-none-elf` and strict `cargo +esp clippy -Zbuild-std=core,alloc --target xtensa-esp32-none-elf` on `--bins --lib`.

Formatting enforcement:

- CI enforces Rust formatting via `cargo +stable fmt --all -- --check` in `.github/workflows/rust_ci.yml` (`PR Light CI` -> `Rust Format` job).

Optional full (online) validation:

```bash
MARKDOWN_LINKS_ONLINE=1 scripts/ci/check_markdown_links.sh --all
```

## Source Architecture Review

Line-count reports are advisory signals for review, not acceptance thresholds.
Split code when doing so creates a coherent responsibility, ownership boundary,
test seam, or hardware-lifetime boundary. Do not split a cohesive module merely
to reduce its line count.

- Blocking code-analysis checks focus on function complexity and argument-heavy
  APIs rather than file length.
- Large-file reports help reviewers find areas worth inspecting, but they do not
  fail CI.
- Prefer folder-based splits over flat suffix files. Example: prefer `src/firmware/event_engine/tap/hsm.rs` and `src/firmware/event_engine/tap/trace.rs` over `src/firmware/event_engine/tap_hsm.rs` and `src/firmware/event_engine/tap_trace.rs`.
- Generated/build outputs are excluded from these reports (for example `target/**` and `**/out/**`).
