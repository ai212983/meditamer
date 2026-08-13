/* Replaces esp-hal's `linkall.x`, selected by `-Tmeditamer-linkall.x` in
 * .cargo/config.toml.
 *
 * Identical to esp-hal's, except it pulls in our own `meditamer-memory.x`
 * instead of the generated `memory.x`. The unique filename matters: esp-hal
 * puts its OUT_DIR ahead of ours on the linker search path, so a file named
 * `memory.x` here would be silently ignored. Everything else still resolves
 * from esp-hal.
 *
 * See docs/reference/dram/dram-budget.md.
 */
INCLUDE "meditamer-memory.x"
INCLUDE "alias.x"
INCLUDE "esp32.x"
INCLUDE "hal-defaults.x"
