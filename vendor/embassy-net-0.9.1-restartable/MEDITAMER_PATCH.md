# Meditamer embassy-net restartable-resource patch

- Upstream crate: `embassy-net` 0.9.1
- License: MIT OR Apache-2.0
- Status: repository-owned Phase 1S feasibility patch

The upstream `StackResources` storage is one-shot: `new()` writes initialized
socket and inner state into `MaybeUninit` fields, but the crate exposes no
destruction/reset contract. Phase 1S reuses one statically allocated resource
block across mutually exclusive Wi-Fi epochs, so overwriting the old state
would leak registered wakers and socket state.

This patch tracks initialization, adds `StackResources::reset()`, destroys the
inner/socket/optional DNS and hostname state in dependency order, calls reset
before every new epoch, and resets on `Drop`. The exclusive mutable borrow
required by `reset()` proves that no `Stack` or `Runner` from the prior epoch
remains live.

Changing the upstream version, feature union, reset order, or resource layout
reopens the Phase 1S source audit.
