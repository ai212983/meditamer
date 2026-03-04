# Feasibility of Sumi‑e Stylised Rendering of a Pre‑baked 3D Scene on ESP32 for a 600×600 E‑ink Display

## Executive summary

Rendering a pre‑baked 3D scene into a sumi‑e / ink‑wash style on an ESP32‑class MCU is feasible **if “rendering” means 2D compositing and stylisation of pre‑baked screen‑space maps** (albedo/light/AO/depth/edge etc.), followed by quantisation and a display update. The hard limits are not “pixel shading” compute, but **(a) the e‑paper update mode/waveform constraints (speed vs greyscale vs ghosting), (b) SRAM/PSRAM availability, and (c) asset bandwidth/layout**. Vendor documentation for IT8951‑based e‑paper illustrates the typical trade: fast modes are often black/white only (A2), while higher‑quality greyscale (GC16) is slower and uses different update behaviour. citeturn12view0turn11view0

On common ESP32/ESP32‑S2/ESP32‑S3 parts there is **no GPU**, but there is enough CPU to run a lightweight “NPR compositor” at 600×600, *provided you avoid large multi‑pass diffusion simulations and avoid holding many full‑resolution maps in internal SRAM at once*. The base ESP32 has 520 KB SRAM and can run up to 240 MHz. citeturn0search4 ESP32‑S2 lists 320 KB on‑chip SRAM (up to 240 MHz). citeturn0search2 ESP32‑S3 is commonly positioned with 512 KB internal SRAM and up to 240 MHz dual‑core. citeturn0search25 External PSRAM (where present) changes the picture dramatically, but comes with cache/throughput restrictions. citeturn14view0

A real product reference point exists: a 600×600 e‑paper device (Inkplate 4 TEMPERA) built around ESP32 hardware is advertised with **fast partial refresh in 1‑bit mode (~0.18 s) and full refresh under ~1 s, plus 3‑bit (8‑level) greyscale capability** on that panel/controller combination. citeturn1search0turn2search17turn2search2 That is unusually quick for e‑paper at this size; many mainstream SPI e‑paper modules are markedly slower (seconds for full refresh, and ~0.4 s class partial refresh on some). citeturn9search14turn11view0

The candid bottom line: **you can get convincing sumi‑e still images (and slow “interactive” parameter changes) on ESP32**, but **not** high‑frame‑rate animated 3D with dynamic shadows and GI. If you want the sumi‑e *look* to be controllable (ink density, fog, edge emphasis), bake the expensive geometry/lighting offline and keep the on‑device work to LUTs, a couple of gradients, and one pass of edge/texture modulation. Ink diffusion can be approximated, but true physically‑based wet‑media simulation is not what you do on an ESP32 at 600×600. citeturn20view0turn18search1turn23search1

## Hardware and display constraints that dominate feasibility

**MCU compute and memory reality.** The baseline ESP32 datasheet lists a single/dual‑core Xtensa LX6 CPU, a maximum frequency of 240 MHz, 520 KB SRAM, and published CoreMark results (useful as a rough throughput sanity check, not a graphics benchmark). citeturn0search4 The ESP32‑S2 datasheet describes 320 KB of on‑chip SRAM and up to 240 MHz. citeturn0search2 The ESP32‑S3 product positioning emphasises dual‑core operation up to 240 MHz and 512 KB internal SRAM. citeturn0search25 None of these parts include a 3D GPU; all pixel processing is CPU.

**External RAM is common but not free.** Many widely used modules ship with PSRAM (for example, some ESP32‑WROVER variants list 8 MB PSRAM and 4/8/16 MB flash). citeturn8view0 In ESP‑IDF, external PSRAM is mapped into the address space and can be allocated via the capability allocator, but **it shares cache behaviour with flash**, becomes **inaccessible when flash cache is disabled**, and large streaming accesses can evict cached code/data. citeturn14view0 For display drivers using SPI DMA, buffers often must live in internal DMA‑capable memory; this pushes you toward strip buffers in internal SRAM even when you have PSRAM for working data. citeturn13search16

**Flash access patterns matter.** ESP‑IDF explicitly supports mapping partitions into address space via `esp_partition_mmap()`, which is attractive for read‑only pre‑baked assets because it avoids copies and enables sequential access (within the constraints of cache/page mapping). citeturn13search0 Practically: you want *streaming‑friendly* asset layouts, ideally with per‑tile locality.

**E‑paper update modes are the real “frame rate”.** With modern controller boards (notably IT8951‑based), e‑paper documentation shows distinct modes:  
- **A2**: fastest, **black/white only**. citeturn12view0  
- **GC16**: 16 greyscale levels for best appearance, typically slower, different waveform. citeturn12view0turn11view0  
Waveshare’s IT8951 e‑paper HAT documentation also states greyscale can be 2–16 (1–4 bits) and that the display retains content without power (a key architectural advantage: you can compute, update, then deep‑sleep). citeturn11view0

**Partial update limits and ghosting are non‑negotiable.** Multiple vendor manuals warn that you cannot do partial refresh indefinitely; after several partial updates you should do a full refresh to remove ghosting, and misuse can produce abnormal effects. citeturn9search2turn11view0turn9search8 This affects pipeline design: “incremental updates” are viable, but you must treat full refresh as periodic maintenance, not an optional extra.

**Concrete reference for 600×600 class hardware.** A commercially documented 600×600 e‑paper device built around ESP32 hardware (Inkplate 4 TEMPERA) lists ~0.18 s partial refresh in 1‑bit mode and ~0.86 s full refresh in 1‑bit and 3‑bit modes, plus 3‑bit greyscale (8 levels). citeturn1search0turn2search17turn2search2 Treat these as *best‑case* numbers for a particular panel/controller/waveform combination, not as a universal law for “any 600×600 e‑ink”.

## What each pre‑baked map enables on-device, and what it costs

A recurring theme in ink‑wash NPR literature is splitting the problem into **feature/line rendering** (silhouettes, creases, structure) and **interior stylisation** (tone, wash, paper texture, diffusion). citeturn10search3turn23search1turn20view0 Your map set is essentially a pre‑baked “G‑buffer” (in screen space or texture space) that lets an MCU approximate that pipeline without geometry processing.

The table below assumes **screen‑space maps at 600×600** (i.e., already rendered from the target camera), because that is the most ESP32‑friendly interpretation of “pre‑baked scene”. If instead you mean UV‑space textures plus on‑device rasterisation, the costs rise sharply (z‑buffer, triangle rasteriser, UV lookup, texture cache). That case is discussed later as an alternative/risk.

### Map-by-map feasibility and effect summary

| Map type | What you can achieve on ESP32 (sumi‑e‑relevant) | What you cannot (or only fake) | Storage at 600×600 (typ.) | On-device cost profile (typ.) |
|---|---|---|---:|---|
| Albedo / diffuse (often greyscale for ink) | Base tonal composition; material separation via tone; can drive wash density and “dry vs wet” look via LUT curves. citeturn20view0turn18search1 | True view‑dependent reflectance; colour‑based effects if you’re strictly monochrome display; specular cues unless baked. citeturn10search3 | 8‑bit: 360 KB | 1 load/pixel + LUT; cheap |
| Lightmap (baked direct+indirect) | Strongest “free realism”: believable shading as a wash; stable chiaroscuro; can emulate ink “five tones” by tone mapping to discrete ink bands. citeturn20view1turn10search3 | Dynamic shadows, time‑of‑day relighting, moving lights, dynamic GI. (You can *crossfade* between multiple baked lightmaps if stored.) | 8‑bit: 360 KB | 1 load + multiply with albedo; cheap |
| Normal map (screen-space or tangent-space) | Approximate **directional relighting** (N·L) to adjust perceived form; can steer stroke direction/anisotropic marks; can detect creases via normal gradients for ink accumulation. NPR systems commonly use geometry buffers for this. citeturn23search7turn10search3 | Self‑shadowing from new light directions; accurate specular; high‑frequency relighting on e‑paper (often dominated by update speed). | 2×8‑bit (oct or XY): 720 KB | 2 loads + dot product + optional gradient; moderate |
| Ambient occlusion | Very effective “ink pooling”: darken cavities, undercuts; helps silhouette readability; supports edge‑darkening masks (dirt/cavity style) which aligns with NPR “interior stylisation” ideas. citeturn10search3turn15search7 | AO does not replace shadows; no dynamic contact changes without rebake. | 8‑bit: 360 KB | 1 load + multiply; cheap |
| Height / depth map (camera depth) | Atmospheric perspective: fog wash, distance fade, soft separation of planes; depth discontinuities for outlines; depth‑weighted stroke coarsening. citeturn10search3turn18search1 | True parallax without geometry; occlusion changes with viewpoint; correct depth if camera moves significantly. | 8‑bit: 360 KB (often enough for fog); 16‑bit: 720 KB | 1 load + LUT for fog; cheap |
| Curvature / edge map (precomputed) | Direct control of “ink accumulation” at ridges/valleys; stable feature lines without running Sobel on-device; thickness‑controlled outlines for sumi‑e ink lines. citeturn23search1turn10search3 | View‑dependent silhouettes if camera changes; edges from dynamic geometry. | 8‑bit: 360 KB | 1 load + subtract/darken; cheap |
| Stylisation masks (material / region masks) | Per‑region control: e.g., keep sky mostly paper white, force mountains to have heavier wash, suppress outlines in mist; critical for art direction. citeturn10search3 | Anything requiring live semantic segmentation unless you compute it. | 1–4 bpp (recommended): 45–180 KB | 1 load; cheap |
| Multi-layer opacity maps (multiple layers) | Foreground/background layering; controllable “ink glaze” overlays; simple parallax by swapping layers. Aligns with multi‑stage compositing used in NPR pipelines. citeturn20view0turn10search3 | True depth‑correct compositing of arbitrary motion; any meaningful number of layers becomes storage‑heavy. | Per layer mask: 45–360 KB depending bpp; multiplied by layers | Alpha blend per pixel per layer; quickly becomes expensive |

### What this table implies

**Albedo + lightmap + AO + depth + edge/mask** is the “sweet spot” for ESP32: it yields a strong sumi‑e look while keeping the per‑pixel work as a handful of integer ops and a couple of LUTs. citeturn10search3turn20view0

**Normal maps are optional**: they unlock “adjustable lighting direction” and better edge detection, but cost a large map plus math. In a slow‑refresh medium like e‑paper, you often do not benefit from dynamic relighting frequently; you benefit from strong static composition. citeturn12view0turn11view0

**Multi-layer opacity is the first thing to cut** unless you have PSRAM *and* a compelling reason (interactive reveal, multi‑scene UI). Each additional layer is both storage and per‑pixel integration cost.

image_group{"layout":"carousel","aspect_ratio":"16:9","query":["sumi-e japanese ink wash landscape painting","sumi-e brush stroke texture close up","e-paper display grayscale close up","normal map texture example"],"num_per_query":1}

## Low-resource rendering pipelines for sumi‑e on ESP32

The most relevant ink‑wash NPR research decomposes into: (a) extracting salient/feature lines, (b) producing interior tones (often tonal abstraction), and (c) adding paper/ink diffusion effects. citeturn10search3turn18search1turn20view0 On ESP32 you should treat (c) as **mostly offline**, and implement a lightweight approximation (micro‑blur plus noise + dithering) only if you can afford it.

### Pipeline flowchart

```mermaid
flowchart LR
  subgraph Offline baking
    A[3D scene + fixed camera(s)] --> B[Render passes: albedo, light, AO, depth, normal, IDs]
    B --> C[Derive stylisation maps: edges/curvature, masks, stroke direction]
    C --> D[Quantise + tile-pack + (optional) compress]
    D --> E[Asset bundle in flash / microSD]
  end

  subgraph ESP32 runtime
    E --> F[Decode tile/strip (small RAM buffers)]
    F --> G[Compose tone: albedo×light×AO]
    G --> H[Depth fog + paper white mixing]
    H --> I[Edge ink accumulation + stroke texture modulation]
    I --> J[Ink wash curve (LUT) + dither/quantise to 1–4 bpp]
    J --> K[Window write to EPD + refresh waveform selection]
  end
```

### A pragmatic on-device stylisation core

The goal is to build a pipeline that is:
- **single-pass per pixel** where possible,
- uses **8‑bit or 16‑bit fixed‑point**, and
- is compatible with **strip/tile rendering** (so you do not need multiple full‑frame buffers).

Key ideas are all standard in NPR/ink rendering: tone mapping to discrete ink bands, feature line darkening, and adding paper/stroke texture. citeturn20view1turn10search3turn23search1

#### Fixed-point conventions

- Represent continuous tones as `uint8` (0–255), where 0 = black ink, 255 = paper white.
- Multiplicative shading: `(a*b + 128) >> 8` (Q0.8).
- Fog factor from depth: precompute a 256‑entry LUT mapping depth8 → fog8 (0–255).
- Tone curve (ink response): another 256‑entry LUT mapping linear tone → stylised tone; this mimics the “ink has non-linear response on absorbent paper” behaviour described in ink wash diffusion discussions. citeturn20view0turn18search1

#### Core composition pseudocode (tile/strip friendly)

```c
// All arrays are "one strip" high: STRIP_H rows, WIDTH columns.
// Each map is 8-bit unless marked otherwise.
// Output is packed to 1bpp or 4bpp depending on selected EPD mode.

for (y = 0; y < STRIP_H; y++) {
  // Optional: keep per-row error buffer for error diffusion dithering
  int16_t err_row[WIDTH + 2] = {0};        // if using error diffusion
  int16_t err_next[WIDTH + 2] = {0};

  for (x = 0; x < WIDTH; x++) {
    uint8_t a  = albedo[y][x];             // 0..255
    uint8_t lm = light[y][x];              // 0..255
    uint8_t ao = ao_map[y][x];             // 0..255
    uint8_t z  = depth[y][x];              // 0..255
    uint8_t e  = edge[y][x];               // 0..255 (0 = none, 255 = strong edge)
    uint8_t m  = mask[y][x];               // 0..255 or bitmask

    // Base shading (Q0.8 multiplies)
    uint16_t t = (a * lm + 128) >> 8;
    t = (t * ao + 128) >> 8;               // still 0..255

    // Depth fog: mix toward paper white
    uint8_t fog = fogLUT[z];               // 0..255 (0 = none, 255 = full fog)
    t = (t * (255 - fog) + 255 * fog + 128) >> 8;

    // Ink accumulation at edges: darken proportional to edge strength
    // edgeStrength is a user-controlled 0..255 scalar
    uint16_t dark = (edgeStrength * e + 128) >> 8;
    t = (t > dark) ? (t - dark) : 0;

    // Brush/paper modulation (tileable texture, cheap)
    // strokeTex returns 0..255 around 128 as neutral
    uint8_t s = strokeTex[(x + u_off) & (TEX_W-1)][(y_global + v_off) & (TEX_H-1)];
    // apply small contrast modulation: t = t + k*(s-128)
    int16_t delta = ((int16_t)s - 128);
    t = clamp_u8((int16_t)t + ((strokeK * delta) >> 8));

    // Nonlinear "ink response" curve
    uint8_t t2 = inkCurveLUT[t];

    // Quantise:
    //  - for 1bpp: ordered dither or error diffusion
    //  - for 4bpp: map to 0..15 via LUT + optional ordered dither
    out[y][x] = quantise(t2, x, y_global);
  }

  // Pack out[y] into the display's format and write the strip window
  epd_write_window(0, y_global, WIDTH, 1, outPackedRow);
}
```

This design lines up with how image-based ink stylisation pipelines are often described: abstraction + edge extraction + diffusion/texture addition, except you are replacing expensive diffusion/texture advection with a small periodic texture and a tone curve. citeturn18search1turn20view0turn23search1

### Edge generation strategies (choose one)

1) **Fully baked edge/curvature maps (recommended).** You bake curvature/ridge intensity offline (or compute it from high‑poly), store as 8‑bit, and simply subtract darkening on device. This mirrors the “feature line rendering” stage that ink NPR papers explicitly separate. citeturn10search3turn23search1

2) **On-device Sobel on depth and/or normals (feasible, but costs RAM).** If you must generate edges dynamically (e.g., you change a light direction and want different crease emphasis), you can compute Sobel using a 3‑row sliding window, which costs 3 scanlines per input map. This is still feasible at 600 px width, but you pay extra flash reads or extra buffering.

3) **Hybrid:** bake a “base edge map” and add a small on-device depth discontinuity edge to catch “mist layers” and UI overlays.

### “Ink diffusion” on ESP32: what is realistic

Physically- or semi‑physically‑based ink diffusion modelling is repeatedly described as complex because it depends on paper structure, water content, pigment transport, etc.; even practical papers often replace full physics with faster image-based approximations. citeturn20view1turn18search1turn10search3 For ESP32:

- **Feasible approximation:** 1–2 passes of separable box blur (or an edge‑aware “limited blur”) on the *already stylised* tone, plus a fine “paper grain” modulation. This yields a mild bleed effect without solving a diffusion PDE.
- **Not feasible in practice:** iterative diffusion with many steps, anisotropic diffusion with costly gradient normalisation, or particle/footprint models intended for GPU pipelines, at full 600×600 every update. citeturn20view0turn23search1turn10search3

### E‑paper update strategy integrated into rendering

You must design with the update waveform in mind:

- If you want **rapid UI-like updates**, target **1‑bit** output and use the display’s fast/partial mode (A2‑like). IT8951 documentation explicitly frames A2 as black/white and fastest. citeturn12view0turn11view0
- If you want **better tonal wash**, target 3‑bit/4‑bit output and accept slower, more disruptive refresh behaviour. The same documentation frames GC16 as 16‑level greyscale for best display effect. citeturn12view0turn11view0
- Track ghosting: vendor docs warn to insert periodic full refresh after multiple partial refreshes. citeturn9search2turn11view0turn9search8

A realistic operational pattern for an art display is:
1. Compose the frame in strips; send window writes.
2. Trigger refresh in the chosen mode.
3. Deep sleep for minutes/hours (e‑paper retains image; power mainly consumed during refresh). citeturn11view0turn1search0

