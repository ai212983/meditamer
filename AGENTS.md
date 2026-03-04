[Meditamer Agent Instructions]|root: ./docs
|IMPORTANT: Prefer retrieval-led reasoning over pre-training-led reasoning
|Uses: Rust, esp-hal, Embassy
|../Inkplate-Arduino-library: Reference C++ library for baseline functionality
|development/README.md: Sharded development guide index (build/flash/monitor/time-sync/soak instructions live in `development/readme/part-*.md`)
|development/upload-throughput-history.md: Sharded index; append new throughput entries to the latest `development/upload-throughput-history/part-*.md`
|development/rfc-upload-throughput-next-phase.md: Sharded index; append new RFC updates to the latest `development/rfc-upload-throughput-next-phase/part-*.md`
|Flash policy: Prefer `scripts/device/flash.sh` over raw `espflash`; use its timeout/fallback diagnostics before deeper debugging
|development/event-engine-guide.md: Practical guide for tuning/modifying the event engine
|development/statig-event-engine-plan.md: Plan for statig-based sensor-event engine
|development/sensors.md: Sensor details and behavior
|development/sound.md: Sound functionality and behavior
|development/hardware-test-matrix.md: Hardware testing matrices
|Documentation policy: Markdown warn at 220 LOC, fail above 300 LOC (`scripts/ci/check_markdown_loc.sh` / `scripts/ci/check_markdown_loc.sh --staged`)
|todos/: Deferred tasks (e.g., cold-boot-validation.md)
|MANDATORY: Never use absolute local filesystem paths/links in tracked files, and never commit them (including generated artifacts, logs, or docs); always use repo-relative paths/links.
|MANDATORY: Do not ignore, bypass, or paper over problems; fix root cause. If unsure, ask the user.
