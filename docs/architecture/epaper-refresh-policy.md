# E-paper refresh policy

## Problem

The region of changed pixels does not explain whether a change needs immediate
feedback or can tolerate the visible blink of a full refresh. Choosing solely
from changed area or a refresh counter loses that distinction.

## Decision

The product supplies an update intent:

- **Fast** requests feedback without a visible full-refresh blink.
- **Clean** requests reconstruction of the display and accepts the blink and
  longer update time. It can be requested even when no pixels have changed.

The display owner combines that intent, the changed region and the panel's state
to choose an operation the hardware can safely perform. An intent is a request,
not a promise that a particular waveform will run immediately.

After a failed operation leaves panel state uncertain, pause ordinary updates
until a clean reconstruction restores a known display state.

## Cost

Callers must choose the intended behavior. Updates can wait for panel recovery,
and clean reconstruction costs time and a visible blink. Automatic area-based
selection would be simpler but cannot express the product's interaction needs.
