/* Included by esp-hal into `.data` when ESP_HAL_CONFIG_USE_RWDATA_LD_HOOK=true.
 *
 * We disable the blanket ESP_HAL_CONFIG_PLACE_SWITCH_TABLES_IN_RAM knob to keep
 * ~13 KB of jump tables in flash, which goes straight to the CPU0 stack (the
 * stack is sized as whatever is left of `dram_seg`). The knob also covers two
 * groups that IRAM-resident code does dereference, so we keep only those here:
 *
 *   - `.rodata.*_esp_hal_internal_handler*`: esp-hal interrupt dispatch tables.
 *   - `.rodata.cst*`: LLVM constant pools. `esp_radio::common_adapter::
 *     semphr_give`/`semphr_take` are `#[ram]` and load one of these, so a
 *     Wi-Fi semaphore op during a cache-disabled flash write would fault if it
 *     lived in flash.
 *
 * `scripts/ci/check_iram_flash_refs.sh` guards this: it fails if any literal in
 * `.rwtext` starts pointing into the flash-mapped rodata window.
 */
*(.rodata.*_esp_hal_internal_handler*)
*(.rodata.cst*)
