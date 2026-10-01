# Meditamer documentation

Documentation is grouped by purpose and lifetime. Choose a section below;
its index or individual documents provide the details.

| I want to… | Start with |
| --- | --- |
| Set up, build, flash, measure, or troubleshoot | [Guides](guides/README.md) |
| Understand the product vision, scope, and UX | [Product](product/) |
| Understand the decisions governing the system | [Architecture](architecture/README.md) |
| Look up current system behavior, hardware, or constraints | [References](references/README.md) |
| Follow unfinished work | [Plans](plans/) |
| Consult historical decisions and closed investigations | [Archive](archive/README.md) |

## Writing conventions

- Place documents by purpose and lifetime: workflows in `guides/`, current
  system contracts in `references/`, decisions in `architecture/`, unfinished
  work in `plans/`, closed investigations in `archive/`.
- Give each rule, field definition and procedure one owner; link to it elsewhere.
  Keep brief operational warnings beside commands when needed to use them safely.
- Archives are frozen, disposable, and excluded from link checks. Keep required
  constraints, rationale and qualification summaries in maintained documentation.
- Use repository-relative paths and links in tracked content, including logs
  and generated artifacts. Do not link outside the repository; reference sibling
  checkouts as plain paths.
- Split on topic boundaries, never into `part-NN.md` files to reduce length.
  Append-only logs and ledgers are exempt from the length advisory.
- `scripts/ci/check_markdown_loc.sh` reports above 220 lines and flags high
  attention above 300; this check is advisory.
- Validate links with `scripts/ci/check_markdown_links.sh` (staged files) or
  `--all`. Vendor documentation is also excluded.
