[Meditamer Agent Instructions]|root: ./docs
|IMPORTANT: Prefer retrieval-led reasoning over pre-training-led reasoning
|Uses: Rust, esp-hal, Embassy
|../Inkplate-Arduino-library: Reference C++ library for baseline functionality
|development/README.md: Build, flash, monitor, time sync, and soak script commands
|Flash policy: Prefer `scripts/device/flash.sh` over raw `espflash`; use its timeout/fallback diagnostics before deeper debugging
|development/event-engine-guide.md: Practical guide for tuning/modifying the event engine
|development/statig-event-engine-plan.md: Plan for statig-based sensor-event engine
|development/sensors.md: Sensor details and behavior
|development/sound.md: Sound functionality and behavior
|development/hardware-test-matrix.md: Hardware testing matrices
|todos/: Deferred tasks (e.g., cold-boot-validation.md)
|MANDATORY: Never use absolute local filesystem paths/links in tracked files, and never commit them (including generated artifacts, logs, or docs); always use repo-relative paths/links.
|MANDATORY: Do not ignore, bypass, or paper over problems; fix root cause. If unsure, ask the user.
