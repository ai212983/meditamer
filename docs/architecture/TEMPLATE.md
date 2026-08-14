# ADR-NNNN: Short imperative title

- Status: Proposed
- Author: <name, or the model that wrote it, e.g. Claude/Opus5-high>
- Date: YYYY-MM-DD
- References: `[ADR-NNNN](NNNN-slug.md)`, `[DRAM budget](../reference/dram/dram-budget.md)`

## Context

What forces are in play. What is true today that makes a decision necessary.
State constraints as facts, with numbers where they exist — DRAM headroom,
refresh timings, flash budget. Avoid arguing for the outcome here.

## Decision

The decision, in the active voice: "We do X." One paragraph if possible.

## Consequences

What becomes easier, what becomes harder, and what this forecloses. Include the
costs honestly — an ADR whose consequences are all positive is not recording a
trade-off, and is usually hiding one.

## Alternatives considered

Each alternative with the reason it lost. "Not considered" is worse than an
empty section: if there was no alternative, say the decision was forced and why.

---

## Conventions

- **Status** is exactly one of `Proposed`, `Accepted`, `Superseded by ADR-NNNN`,
  or `Deprecated`. Qualifiers go in the body, not the status line.
- **Numbering** is sequential and never reused, including for withdrawn ADRs.
- **Amendments**: add an `- Amended: YYYY-MM-DD` line for clarifications that do
  not change the decision. A reversal is a new ADR that supersedes this one;
  never rewrite an accepted decision in place.
- **Author** is required on new ADRs. ADRs 0001–0008 pre-date this convention
  and are unattributed; do not backfill them with a guess.
