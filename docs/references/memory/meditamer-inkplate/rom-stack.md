# Inkplate ROM-stack constraints

The [linker override](../../../../config/linker/esp32/meditamer-memory.x) reclaims
`0x3FFE4350..0x3FFE7E30` (15,072 bytes) for passive framebuffer storage.
ROM data stays reserved; the PRO region `0x3FFE0440..0x3FFE3F20` remains excluded.

## Required placement

- Use `NOLOAD`; startup may overwrite this memory before the application owns it.
- Pin `.dram2_uninit.framebuffer` before `.dram2_uninit.heap`. The 45,000-byte
  framebuffer covers the reclaimed window; heap must begin at or above `0x3FFE7E30`.
- Give every additional static its own input section and explicit ordering.
  Incidental link order is not an ownership contract.
- Keep the override through `meditamer-linkall.x`; a project `memory.x` can be
  shadowed by esp-hal's linker search order. Run the pinned-linker guard after upgrades.

## Evidence supporting the constraint

On 2026-08-21, a same-source/same-board A/B changing only the two input-section
names recorded 0/40 boot panics with heap above the window and 12/12 with heap
overlapping it. Every control panic reported `EXCVADDR=0x4000c0d4`.
Earlier 40-boot tests recorded 11 failures with PRO-window heap and 13 with
APP-window heap. This supports the placement restriction; it does not establish
general safety of reclaimed heap memory or qualify today's radio stack.

Do not add reclaimed ROM-stack heap without new startup/lifetime evidence.
Requalification follows [memory validation](../../../guides/memory/validation.md#placement-and-runtime-qualification).

## Sleep

Deep sleep resets the chip; ordinary internal SRAM and PSRAM state must be
reconstructed. Cross-wake state needs flash or explicitly configured RTC retention.
Light sleep does not restart ROM boot. After reconstructing framebuffers, use a
full panel refresh before partial diffing against the retained physical image.
