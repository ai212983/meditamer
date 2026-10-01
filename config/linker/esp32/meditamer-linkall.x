/* Replaces esp-hal's `linkall.x`, selected by `-Tmeditamer-linkall.x` in
 * .cargo/config.toml.
 *
 * Pulls in our own `meditamer-memory.x` instead of the generated `memory.x`
 * and supplies three ESP32 ROM mappings missing from esp-rom-sys 0.1.5.
 * The unique filename matters: esp-hal puts its OUT_DIR ahead of ours on the
 * linker search path, so a file named `memory.x` here would be silently
 * ignored. Everything else still resolves from esp-hal.
 *
 * See docs/references/memory/meditamer-inkplate/budget.md.
 */
INCLUDE "meditamer-memory.x"
INCLUDE "alias.x"
INCLUDE "esp32.x"
INCLUDE "hal-defaults.x"

/* esp-radio 1.0.0-beta.1's BTDM blob uses these ESP32 ROM entry points, but
 * its exact esp-rom-sys 0.1.5 dependency predates their mappings. These are
 * the ROM addresses added upstream in esp-rs/esp-hal#6271, not callback stubs.
 * Remove them once the resolved esp-rom-sys release provides the mappings.
 */
PROVIDE ( ld_iscan_evt_start_cbk = 0x4003b58c );
PROVIDE ( ld_page_evt_start_cbk = 0x4003cf40 );
PROVIDE ( ld_pscan_evt_start_cbk = 0x4003e924 );
