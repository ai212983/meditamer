# meditamer

The development guide is sharded to keep Markdown files under the documentation LOC policy.

## Guide Parts

- [Part 01: Runtime stack, references, and hooks](./readme/part-01.md)
- [Part 02: Build and flash workflows](./readme/part-02.md)
- [Part 03: Runtime metrics](./readme/part-03.md)
- [Part 04: Wi-Fi upload acceptance/regression workflow](./readme/part-04.md)
- [Part 05: Troubleshoot workflow](./readme/part-05.md)
- [Part 06: Advanced Wi-Fi diagnostics](./readme/part-06.md)
- [Part 07: Runtime setup and service modes](./readme/part-07.md)

## Notes

- Add new content to the matching part file under `docs/development/readme/`.
- Start a new part when a file approaches `220` LOC.
- Keep this file as an index only. Do not append long-form operational content here.
- Flash/boot-capture usage lives in `docs/development/readme/part-02.md`.
- Hostctl Serverless Workflow authoring guidance lives in `docs/development/hostctl-workflow-authoring.md`.
- For Wi-Fi/upload tuning preflight, check `docs/development/wifi-upload-decision-ledger.md` first.
- For the current Wi-Fi migration direction, check
  `docs/development/wifi-legacy-vendoring-plan.md` before starting new
  root-cause experiments.

## Editing Sharded Docs

- Throughput history updates:
  - append to the latest file in `docs/development/upload-throughput-history/part-*.md`
    (currently `part-30.md`)
  - when the latest part approaches `220` LOC, create the next part and update
    `docs/development/upload-throughput-history.md` links
- RFC updates:
  - append to the latest file in
    `docs/development/rfc-upload-throughput-next-phase/part-*.md`
  - when the latest part approaches `220` LOC, create the next part and update
    `docs/development/rfc-upload-throughput-next-phase.md` links
- Development guide updates:
  - edit the relevant file under `docs/development/readme/part-*.md`
  - keep `docs/development/README.md` as index/navigation only
