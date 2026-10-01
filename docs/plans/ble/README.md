# BLE plans

Status: Active

BLE work covers Meditamer/Inkplate and Medinote/Waveshare. Shared capabilities
have separate target integration and qualification; either target can progress independently.

The [diagnostic protocol](../../references/ble-diagnostic-protocol.md) describes
the current service. The [Inkplate handoff procedure](../../guides/network/validation.md#inkplate-wi-fible-handoff)
owns its runnable check; [power criteria and completed evidence](../../references/radio-qualification.md)
live in the qualification reference.

| Plan | Remaining work |
| --- | --- |
| [Central input and device support](central-input.md) | Persistent bonds, reconnect, common Q37/CheerTok drivers and product integration |
| [Radio policy and qualification](radio-policy-and-qualification.md) | Configurable default owner and integrated target qualification |
| [Diagnostics and promotion](diagnostics.md) | Shared peripheral service, Waveshare integration and promotion |

## Deferred

- **Simultaneous BLE roles:** controller input alongside a phone/computer peripheral.
  Requires single-role qualification on the target. This is separate from Wi-Fi/BLE coexistence.
- **BLE asset upload:** optional adapter to the shared storage owner; Wi-Fi HTTP
  remains primary. Requires qualified peripheral/storage integration and a decision
  on authorization, integrity and transfer recovery. Deliver macOS first, iPhone later.
