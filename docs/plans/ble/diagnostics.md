# Diagnostics and promotion

Status: Active

Provide one shared diagnostic peripheral for both products. The
[protocol reference](../../references/ble-diagnostic-protocol.md) describes the
current Inkplate implementation; the work below includes intended changes.

## Work

- Extract the live service with product identity and target lifecycle ports;
  wire it into both targets without forking the wire protocol.
- Use the actual manifest digest prefix in Build Info.
- Complete configuration validation, subscription/permission enforcement and
  notification failure handling while preserving quotas, deadlines and close precedence.
- Accept Waveshare diagnostic identity, resource and power policy.
- Document operation, compatibility and troubleshooting in the owning guides.

## Checks

Verify framing, subscriptions, backpressure, identity/cache recovery and security
with a real BLE client. Read Build Info and Lifecycle Status, subscribe to Echo,
and check returned payloads and rejection of invalid or over-quota writes against
the protocol. Record client/tool identity and results.

The [Inkplate handoff run](../../guides/network/validation.md#inkplate-wi-fible-handoff)
does not perform those client checks. Establish client and power evidence on each
target, then complete [integrated qualification](radio-policy-and-qualification.md#qualification).
