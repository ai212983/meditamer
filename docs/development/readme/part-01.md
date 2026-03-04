# meditamer

## Runtime Stack

The firmware now targets `esp-hal` (`xtensa-esp32-none-elf`) as the primary and only runtime path.

## Documentation Reference

- `statig-event-engine-plan.md`: phased migration plan for replacing in-task heuristic tap logic with a `statig`-based generic sensor-event engine.
- `event-engine-guide.md`: practical developer guide for tuning and modifying the current event engine implementation.
- `sensors.md`: Sensor details and behavior.
- `sound.md`: Sound functionality and behavior.
- `hardware-test-matrix.md`: Hardware testing matrices.
- `reliability-issues.md`: Current ranked reliability risks, evidence, and mitigation gates.
- `wifi-discovery-regression-guardrails.md`: Why zero-discovery can regress and how to prevent it.
- `wifi-upload-regression-protocol.md`: Panic-first containment, triage, artifact, and closure protocol for Wi-Fi/upload regressions.
- `troubleshoot-agent.md`: Agent-first runbook for the Serverless Workflow troubleshooting script.

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
- Runs host-tooling clippy via `scripts/ci/lint_host_tools.sh` (`-D warnings`) when staged files touch `tools/**` or workspace toolchain manifests.

Current commit-msg hook:

- Validates commit messages against Conventional Commits via `scripts/ci/check_commit_message.sh` and `commitlint`.
- Requires a scope in `type(scope): subject` format.
- Enforces allowed scopes: `runtime`, `touch`, `event-engine`, `storage`, `upload`, `wifi`, `telemetry`, `graphics`, `drivers`, `tooling`, `ci`, `docs`.
- Exempts Git-generated/autosquash subjects (`Merge ...`, `Revert ...`, `fixup! ...`, `squash! ...`) from custom scope checks.

Current pre-push hook:

- Runs strict firmware clippy via `cargo clippy --locked --all-features --workspace --bins --lib -- -D warnings` when pushed files touch firmware/workspace Rust paths.
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

- The firmware is `no_std` with heavy feature/cfg gating; analyzer results can include inactive-code and unresolved-import noise outside active build paths.
- The baseline script intentionally runs with `--disable-build-scripts --disable-proc-macros` for stable, fast CI signal.
- Authoritative correctness gates remain `cargo check` and strict `cargo clippy` on `--bins --lib`.

Formatting enforcement:

- CI enforces Rust formatting via `cargo +stable fmt --all -- --check` in `.github/workflows/rust_ci.yml` (`PR Light CI` -> `Rust Format` job).

Optional full (online) validation:

```bash
git ls-files -z '*.md' | xargs -0 env MARKDOWN_LINKS_ONLINE=1 scripts/ci/check_markdown_links.sh
```

## Markdown LOC Policy

- warning threshold: `220` lines
- hard limit: `300` lines
- local staged check: `scripts/ci/check_markdown_loc.sh --staged`
- full repo check: `scripts/ci/check_markdown_loc.sh`
- CI workflow: `.github/workflows/docs_ci.yml`
- current exclusions:
  - `docs/archive/**`
  - `tools/**/deep-research-report*.md`

When a document approaches the warning threshold, split it into shard parts and
keep the original path as a short index page linking to those parts.

## Sharded Docs Workflow

- Throughput log updates go to the latest shard in `docs/development/upload-throughput-history/part-*.md`.
- RFC updates go to the latest shard in `docs/development/rfc-upload-throughput-next-phase/part-*.md`.
- Development-guide operational updates go to the relevant `docs/development/readme/part-*.md` file.
- Keep these as index pages only: `docs/development/upload-throughput-history.md`, `docs/development/rfc-upload-throughput-next-phase.md`, `docs/development/README.md`.
- When a latest shard nears `220` LOC:
  - create the next `part-XX.md`
  - add it to the index page links in order.

## File Size Guidelines (Rewrite Phase)

These limits are active during the current rewrite on this branch. Enforcement is manual in review for now (no hooks yet).

- Hard cap: non-generated source files must stay at or below `500` lines.
- Split-plan trigger: once a file crosses `420` lines, the same PR must include a short split plan.
- Warning threshold: treat `450` lines as "split now unless there is a blocking reason".
- New modules target: keep new modules at or below `300` lines.
- Prefer folder-based splits over flat suffix files. Example: prefer `src/firmware/event_engine/tap/hsm.rs` and `src/firmware/event_engine/tap/trace.rs` over `src/firmware/event_engine/tap_hsm.rs` and `src/firmware/event_engine/tap_trace.rs`.
- Generated/build outputs are excluded from these limits (for example `target/**` and `**/out/**`).

Suggested PR checklist line:

- `[]` If any touched file is `>= 420` lines, I included a split plan in this PR description.

## Display Runtime Behavior

- Clock refresh task: every 5 minutes (`300s`)
- Battery task: independent Embassy task every 5 minutes (`300s`)
- Battery label: top-right (`BAT xx%`)
- Battery percentage source: BQ27441 fuel gauge `SoC` register (reference behavior)

## Build

```bash
scripts/build/build.sh [debug|release]
```

Default is `release` when no argument is provided.

The default Xtensa runner (`scripts/build/xtensa_runner.sh`) flashes firmware without opening
an interactive monitor (safe in non-interactive shells). To enable monitor explicitly:

```bash
ESPFLASH_RUN_MONITOR=1 cargo run
```
