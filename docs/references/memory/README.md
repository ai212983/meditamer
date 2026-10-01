# Memory budgets

Budgets belong to firmware targets (product + board), with separate build
configurations where placement or reserves differ.

| Target                                               | Production budget baseline                                            |
|------------------------------------------------------|-----------------------------------------------------------------------|
| [Meditamer / Inkplate](meditamer-inkplate/budget.md) | BLE-linked firmware with Wi-Fi; becoming the default                  |
| [Medinote / Waveshare](medinote-waveshare/budget.md) | `wifi-storage` with the default BLE/UI features; becoming the default |

Statics and Embassy task pools in main DRAM reduce the linked CPU-stack remainder.
Heap backing and task pools are already included in their owning sections;
do not count them twice. Linked reserves and generated call frames do not
establish runtime peaks. Free bytes do not guarantee a contiguous allocation.
Internal/DMA/cache-disabled requirements constrain PSRAM placement.

Use [memory validation](../../guides/memory/validation.md) for measurements.
Keep required constraints, rationale and qualification summaries here, independent
of disposable archives and logs. Build reports own measured section totals;
do not maintain a per-change ledger or duplicate those totals in these documents.
