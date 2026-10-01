# Platform, board, product and target boundaries

## Problem

Shared code becomes difficult to reuse when it also decides product behavior or
constructs a particular device's hardware.

## Decision

Divide responsibility into four parts:

| Part | Owns |
| --- | --- |
| Platform | Reusable services and interfaces |
| Board | Hardware drivers and operations |
| Product | Application behavior, content and presentation |
| Target | Startup, hardware construction and wiring the parts into firmware |

Platform code does not depend on a board, product or target. Board code may use
platform interfaces, but does not choose application behavior. Products may use
platform services; they do not depend on target startup code. Targets connect the
concrete implementations.

A product that depends directly on board types accepts reduced portability.
That dependency does not allow board or platform code to depend on the product.

Shared UI code defines lifecycle and interaction rules. Each product can choose
its own controls and widgets; sharing those rules does not require identical UI.

Keep input and sensor acquisition separate from rendering. Acquisition owns
peripheral access and publishes results through messages, so sampling progress
does not depend on the rendering loop.

## Cost

Explicit interfaces and target wiring require more integration code. In return,
changing hardware or presentation does not require moving product rules into
shared services.

Separate acquisition also requires communication buffers and runtime state.
