# Architecture decisions

New ADRs start from [TEMPLATE.md](TEMPLATE.md), which also records the status
vocabulary, numbering, and amendment rules.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-fully-async-touch-acquisition.md) | Fully async touch acquisition | Accepted |
| [0002](0002-phased-async-sd-spi.md) | Phased async SD SPI | Superseded by [0004](0004-dma-stepped-fat-engine.md) |
| [0003](0003-adaptive-async-imu-acquisition.md) | Adaptive async IMU acquisition | Accepted |
| [0004](0004-dma-stepped-fat-engine.md) | DMA-only SPI and stepped FAT engine | Accepted |
| [0005](0005-isolate-touch-acquisition-on-core-1.md) | Isolate touch acquisition on core 1 | Accepted |
| [0006](0006-flash-overlay-app-modules.md) | Evaluate native flash-overlay app modules | Deprecated by [0013](0013-compiled-only-ui-catalogue.md) |
| [0007](0007-ui-and-application-structure.md) | UI shell and application structure | Accepted |
| [0008](0008-app-catalogue-and-launcher.md) | App catalogue and launcher | Accepted |
| [0009](0009-ab-firmware-update-foundation.md) | Signed A/B firmware-update foundation | Superseded by [0014](0014-single-production-sd-recovery-updater.md) |
| [0010](0010-durable-ui-settings.md) | Durable UI settings transaction | Accepted |
| [0011](0011-bounded-ble-service-foundation.md) | Bounded coordinated BLE service foundation | Proposed |
| [0012](0012-sdcard-package-boundary.md) | Retain the `sdcard` package boundary; no Wi-Fi package extraction | Superseded by [0015](0015-two-product-platform-workspace.md) |
| [0013](0013-compiled-only-ui-catalogue.md) | Keep the UI catalogue compiled-only | Accepted |
| [0014](0014-single-production-sd-recovery-updater.md) | Replace A/B with a factory updater and one production image | Accepted |
| [0015](0015-two-product-platform-workspace.md) | Split the tree into platform, board, and product axes in one workspace | Accepted |

Execution evidence for ADR-0006 through ADR-0010 is preserved in the archived
[UI and app structure rework plan](../archive/refactors/ui-app-structure-rework-plan.md) and its
[implementation ledger](../archive/refactors/ui-app-structure-rework-ledger.md).

ADRs 0001–0008 pre-date the `Author:` convention and are unattributed. Do not
backfill them with a guess; new ADRs must carry the line.
