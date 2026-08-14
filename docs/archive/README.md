# Archive

Closed investigations, kept as evidence. **Frozen: do not edit anything here.**

These files are excluded from link validation and from the Markdown LOC
advisory. Their internal cross-references were written against the layout they
had while live and are not maintained. Live documents may link *into* this
directory; nothing here should be treated as current guidance.

| Directory | What it holds | Why it is kept |
| --- | --- | --- |
| [`wifi/`](wifi/) | The zero-discovery blackout hunt: ~70 dated narrowing notes, the decision ledger, the regression protocol, and the [blackout diagnostic knobs](wifi/blackout-diagnostic-knobs.md) | The experiment novelty gate in `AGENTS.md` requires searching it before re-running any Wi-Fi knob |
| [`upload/`](upload/) | Upload throughput history and the upload RFC, both sharded into `part-NN.md` | Same novelty gate; also the measurement record behind current throughput expectations |
| [`research/`](research/) | Early product-vision research reports and brainstorming | Background for `docs/product/`; superseded by it |
| [`refactors/`](refactors/) | Completed structural refactor plans, with their outcome, deviations, and device evidence | The record of why the module tree is shaped the way it is, and what was measured to prove each move safe |
| [`host-tooling/`](host-tooling/) | Completed host-tooling plans and execution ledgers | Evidence behind durable commands and operating policies documented in `docs/guides/` |

## Before re-running a Wi-Fi or upload experiment

The blackout is fixed. Nearly every knob recorded here ends in "keep default
off" or "rejected root-cause branch". Search before you re-derive:

```bash
rg -n '<knob-or-value>' docs/archive/wifi docs/archive/upload
```

The surviving live guard is
[docs/guides/wifi-regression-gate.md](../guides/wifi-regression-gate.md).

## Archiving something new

Move it here whole, in one commit, when its investigation closes — do not
delete it and do not trim it. Add a row above if it starts a new category, and
fix any live document that linked to it.
