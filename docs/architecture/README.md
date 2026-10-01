# Architecture decisions

Author: Codex

These documents explain specific design choices: the problem, the chosen approach
and its cost. They are not inventories of the current firmware or proof that an
implementation has passed validation.

| Decision | Choice |
| --- | --- |
| [Platform, board, product and target boundaries](platform-board-product-target-boundaries.md) | Keep reusable services, hardware, product behavior and firmware wiring separate |
| [Wi-Fi/BLE radio exclusivity](wifi-ble-radio-exclusivity.md) | Run Wi-Fi and BLE sequentially under one supervisor |
| [E-paper refresh policy](epaper-refresh-policy.md) | Let products request Fast or Clean; let the display owner choose the physical operation |

## Maintaining a decision

Name the concrete problem and the choice made to solve it. Explain what that
choice costs and what would need reconsideration if it changed. Keep unrelated
choices in separate documents.

Do not label a snapshot or an unverified proposal as an accepted decision. Keep
measurements and implementation procedures with their evidence rather than
compressing them into architectural rules.
