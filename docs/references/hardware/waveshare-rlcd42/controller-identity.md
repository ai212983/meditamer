# Which controller is actually on the RLCD?

Waveshare's driver component calls it **ST7305**, and ships the ST7305 datasheet. Zephyr's board
port declares **`sitronix,st7306`**. The register evidence says Zephyr is right and both vendors
are mislabelling it.

This matters less than it looks — see [What follows from it](#what-follows-from-it) — but it decides
which datasheet you can trust, and the ST7305 one is actively misleading.

## The evidence

Every geometry limit the ST7305 datasheet documents is exceeded by the vendor's own init sequence.
Every one of them fits inside the ST7306's.

| | This board's init | [ST7305 V0.2](datasheets/st7305-datasheet.pdf) | [ST7306 V0.1](datasheets/st7306-datasheet.pdf) |
| --- | --- | --- | --- |
| CASET (`0x2A`) | `0x12`..`0x2A` (18–42) | `19 ≤ XS < XE ≤ 40` — **out at both ends** | `0 ≤ XS < XE ≤ 59` — fits |
| RASET (`0x2B`) | `0x00`..`0xC7` (0–199) | `0 ≤ YS < YE ≤ 159` — **exceeded** | `0 ≤ YS < YE ≤ 239` — fits |
| GATESET (`0xB0`) | `0x64` → GL 100 → **400** lines | table ends at 320 — **exceeded** | table ends at 480 — fits |
| Frame memory | 300 x 400 x 1b = 15,000 B | "264 x 320 x 1b internal SRAM" — **too small** | "720 x 480 x 1b internal SRAM" — fits |

And the detail that turns a pattern into a fingerprint: the driver mirrors its column window as
`0x3C - addr`, i.e. within a **60-address** space. The ST7306 has exactly 60 column addresses
(`0`..`59`). The ST7305 has 22 usable ones (`19`..`40`). A constant that arbitrary matching a
documented address space that exactly is not coincidence.

`0x3A` corroborates it. On the ST7305 that register carries two bits, `XDE` and `BPS`. On the ST7306
it also carries `M8C`, the colour-mode select — and the vendor init writes `0x11`, which sets bits
the ST7305 does not define.

## What the vendors say, and why it is weak

Both Waveshare and GooDisplay label this panel ST7305 — GooDisplay sells the same geometry as
[GDTL042T71](https://www.good-display.com/product/455.html), "4.2 inch, 300(H) x 400(V), 4-line SPI,
driver IC ST7305", and pairs it with the same `ST_7305_V0_2.pdf`. Two independent vendors agreeing
looks like strong evidence until you notice they are shipping a datasheet whose own address ranges
their panel violates. Panel vendors label by product family; the register map is objective.

There is no newer ST7305 datasheet that would resolve this by covering 300x400: GooDisplay's
"ST7305 IC Datasheet" download is byte-for-byte the same V0.2 document Waveshare mirrors.

## The direct test failed

Reading the controller's ID registers would have settled it outright. It does not work on this
board: [`panel_id_probe.rs`](../../../../targets/medinote-waveshare/src/bin/panel_id_probe.rs) read
`RDDID` and `RDID1`/`RDID2`/`RDID3` at three points in bring-up, each twice with opposite bus pulls,
and **all twelve reads tracked the pull exactly** (`up=0xFFFFFFFF down=0x00000000`). Nothing drives
SDA back.

The divergence between the two pulls proves the probe's own turnaround worked — the ESP32-S3 output
driver really was released, or the pulls could not have mattered — so the negative is about the
board, not the probe. What it cannot distinguish is a die that does not support reads from a module
that does not wire the return path.

Evidence: `logs/panel_id_probe_20260828T122039Z/`, ELF SHA-256 `1bf4f033…`, board MAC
`a4:cb:8f:d0:6a:74`, 2026-08-28.

## What follows from it

**Read the ST7306 datasheet, not the ST7305 one**, for anything touching geometry, address ranges,
or RAM. The ST7305 document is still fine for the parts the two share — the serial protocol, MADCTL,
and the FRCTRL/OSCSET decoding behind [the frame-rate table](panel.md#frctrl-is-self-refresh-not-write-latency),
which measured correctly against it.

**Greyscale is probably still out of reach, so this changes less than it seems.** The ST7306 is the
grey-capable sibling, which is the one capability that would justify caring about the part number —
but its grey modes cap at "360H x 240V 10Grey/16Grey", and this panel is 300H x **400V** native. The
height exceeds the grey-mode limit, so full-screen greyscale is not available even if the die
supports it. A reduced-height grey region might be; nobody has tried.

**The code keeps saying ST7305 for now.** `St7305` in
[panel.rs](../../../../boards/waveshare-rlcd42/src/panel.rs) is named after what the vendor driver
it was translated from calls the part. Renaming the type is a mechanical change worth doing only
alongside something that benefits from it; what matters more is that the driver's own comments point
here.

**What would close it for good:** the marking on the panel's flex, or a direct answer from Waveshare
support. Both are cheaper than any further firmware work now that the read path is known dead.
