[Meditamer Agent Instructions]|root: ./docs
|IMPORTANT: Prefer retrieval-led reasoning over pre-training-led reasoning
|Uses: Rust, esp-hal, Embassy
|../Inkplate-Arduino-library: Reference C++ library for baseline functionality
|development/README.md: Sharded development guide index (build/flash/monitor/time-sync/soak instructions live in `development/readme/part-*.md`)
|development/upload-throughput-history.md: Sharded index; append new throughput entries to the latest `development/upload-throughput-history/part-*.md`
|development/rfc-upload-throughput-next-phase.md: Sharded index; append new RFC updates to the latest `development/rfc-upload-throughput-next-phase/part-*.md`
|development/wifi-upload-decision-ledger.md: Fast decision ledger for promoted/rejected/non-default Wi-Fi/upload knobs; check this first before proposing/rerunning A/B experiments
|MANDATORY (experiment novelty gate): Before any Wi-Fi/upload A/B or tuning rerun, search `development/rfc-upload-throughput-next-phase/` and `development/upload-throughput-history/` for the exact knob/value. If it is already completed/rejected, do not rerun unless the user explicitly asks for reconfirmation.
|Flash policy: Prefer `scripts/device/flash.sh` over raw `espflash`; it wraps `hostctl flash-capture` and the canonical orchestration in `tools/hostctl/scenarios/flash-capture.sw.yaml`. Use `hostctl flash-capture` directly only when you need explicit `--flash-mode` / `--capture-mode` / artifact-path control. Use `scripts/device/monitor.sh` only for passive attach/debug, not boot capture.
|Hostctl workflow policy: Keep orchestration (branching, fallback order, retries, gate flow) in Serverless Workflow YAML under `tools/hostctl/scenarios/*.sw.yaml`; keep Rust hostctl code focused on primitive actions and context I/O.
|development/event-engine-guide.md: Practical guide for tuning/modifying the event engine
|development/statig-event-engine-plan.md: Plan for statig-based sensor-event engine
|development/sensors.md: Sensor details and behavior
|development/sound.md: Sound functionality and behavior
|development/hardware-test-matrix.md: Hardware testing matrices
|Documentation policy: Markdown warn at 220 LOC, fail above 300 LOC (`scripts/ci/check_markdown_loc.sh` / `scripts/ci/check_markdown_loc.sh --staged`)
|todos/: Deferred tasks (e.g., cold-boot-validation.md)
|MANDATORY: Never use absolute local filesystem paths/links in tracked files, and never commit them (including generated artifacts, logs, or docs); always use repo-relative paths/links.
|MANDATORY: Do not ignore, bypass, or paper over problems; fix root cause. If unsure, ask the user.
