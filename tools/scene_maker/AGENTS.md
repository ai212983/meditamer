# Scene Maker Agent Rules

## Host Rendering Priority
When running scene baking/rendering on a host machine (desktop/laptop), prioritize output quality over performance, storage size, and processing time.

Implications:
- Prefer higher-quality maps, larger intermediate precision, and stronger geometry/detail preservation.
- Do not downscale or simplify purely for speed or disk savings unless explicitly requested.
- Use quality-oriented defaults first; optimization for device constraints is a separate, explicit step.
