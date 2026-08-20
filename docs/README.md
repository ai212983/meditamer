# Meditamer documentation

Five kinds of document, split by how they age. Put a new file where its
*lifetime* fits, not where its topic fits.

| Directory | Holds | Lifetime |
| --- | --- | --- |
| [`product/`](product/) | Vision, MVP scope, UX principles | Changes when the product does |
| [`architecture/`](architecture/) | ADRs — decisions and their consequences | Immutable once accepted; superseded, never edited |
| [`reference/`](reference/) | How the system *is*: budgets, peripherals, test matrices | Durable; updated in place as the system changes |
| [`guides/`](guides/) | How you *work on it*: build, flash, measure, debug | Durable; updated in place as the workflow changes |
| [`plans/`](plans/) | Work not finished yet | Ends as done, superseded, or parked |
| [`notes/`](notes/) | Dated findings, feasibility studies, experiments | Disposable; archive or delete once absorbed |
| [`archive/`](archive/) | Closed investigations, kept as evidence | Frozen — never edit, never link *out of* |

## Start here

- New to the repo → [guides/development-setup.md](guides/development-setup.md)
  (full workflow index: [guides/README.md](guides/README.md))
- Building or flashing → [guides/build-and-flash.md](guides/build-and-flash.md)
- Shipping a signed image → [guides/firmware-update.md](guides/firmware-update.md)
- Something is broken → [guides/troubleshooting.md](guides/troubleshooting.md)
- Adding a large static or buffer → [reference/dram/dram-budget.md](reference/dram/dram-budget.md) **first**
- Changing Wi-Fi, network, or upload → [guides/wifi-regression-gate.md](guides/wifi-regression-gate.md)
- Making a decision worth remembering → [architecture/](architecture/)

## Reference

| Document | Read it before |
| --- | --- |
| [dram-budget.md](reference/dram/dram-budget.md) | Adding statics, task-local buffers, or channel depth |
| [dram-budget-rom-stack.md](reference/dram/dram-budget-rom-stack.md) | Touching `config/linker/esp32/` or adding deep sleep |
| [compile-time-features.md](reference/compile-time-features.md) | Adding or changing a Cargo feature |
| [event-engine-guide.md](reference/event-engine-guide.md) | Tuning tap/gesture behavior |
| [sensors.md](reference/hardware/inkplate/sensors.md) / [sound.md](reference/hardware/inkplate/sound.md) | Working with the IMU, ambient sensors, or buzzer |
| [hardware-test-matrix.md](reference/hardware-test-matrix.md) | Claiming hardware coverage |
| [reliability-issues.md](reference/reliability-issues.md) | Arguing about what is actually risky |
| [display-refresh.md](reference/display-refresh.md) | Touching panel refresh modes or the panel-power lease |
| [font-legibility.md](reference/font-legibility.md) | Changing UI fonts or the dither threshold |

Datasheets live in [reference/hardware/inkplate/datasheets/](reference/hardware/inkplate/datasheets/).

## Conventions

**Plans carry a status header.** Every file in `plans/` starts with `Status:`
and `Last-reviewed:` so a reader can tell live work from parked work without
reading git log. Statuses are `Active`, `Proposed`, `Deferred`, `Superseded`,
`Done`, or `Needs-triage` — the last meaning nobody has confirmed whether the
work is still wanted. Clear those rather than letting them accumulate.

**ADRs are numbered and immutable.** Amend by adding an `Amended:` date for
clarifications; supersede with a new ADR for reversals. Each carries an
`Author:` line naming the human or model that wrote it.

**Markdown length is advisory, and sharding is not the remedy.** Warn above 220
lines, high-attention above 300 (`scripts/ci/check_markdown_loc.sh`). Findings
never block CI. Do **not** split a long document into `part-NN.md` files: the
split adds a hand-maintained index that rots while the document stays one
document. Split only where a real topic boundary exists, and name the pieces
after their topics. Append-only material (logs, ledgers, run journals) is
exempt — let it grow, then archive it when the investigation closes.

**Links are checked.** `scripts/ci/check_markdown_links.sh` runs on staged
files in pre-commit; `--all` validates the whole repo. `archive/` and `vendor/`
are excluded. Never use absolute local paths, and do not link outside the repo
root — reference sibling checkouts as plain paths instead.

**Archive is one-way.** Live docs may link into `archive/`; archived docs are
frozen and their internal links are not maintained. Archive a document when its
investigation closes rather than deleting it — the Wi-Fi and upload series are
kept specifically for the experiment novelty gate in `AGENTS.md`.
