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

## Performance estimates and option comparison

### Raw memory arithmetic you can bank on

For a 600×600 image:
- 1 bpp frame buffer: **45,000 bytes**
- 3 bpp (8 levels): **135,000 bytes**
- 4 bpp (16 levels): **180,000 bytes**
- 8 bpp: **360,000 bytes**
- 16 bpp: **720,000 bytes**

These numbers drive feasibility more than “MIPS”. They tell you that **a full 4‑bpp output buffer fits comfortably in internal SRAM** even on parts without PSRAM (but you may still choose strip rendering to keep headroom for code/Wi‑Fi/FS). The map set, however, is what pushes you to streaming.

### Representative options table

The estimates below assume a 240 MHz class ESP32 core clock and integer math, with strip rendering so that peak internal SRAM is dominated by a strip output buffer plus a few scanlines of inputs. CPU time is given as an order‑of‑magnitude range because real throughput depends on flash/PSRAM placement, compiler, and whether you use Wi‑Fi concurrently. ESP32‑class frequency/memory baselines come from the ESP32 datasheet; external RAM behaviour from ESP‑IDF docs. citeturn0search4turn14view0turn13search0

| Option | Stored assets (600×600 unless noted) | Peak RAM strategy | Flash / SD footprint (approx.) | CPU time for stylisation (approx.) | Display update time driver | When it makes sense |
|---|---|---:|---:|---:|---|---|
| Pre-stylised frames | Final 1–4 bpp images only (no maps) | Single full buffer (45–180 KB) | 45–180 KB per frame | ~10–50 ms (depack + send) | Dominated by panel | Absolute simplest, best reliability; no runtime artistic controls |
| Basic sumi‑e compositor | Albedo + light + AO + depth + edge + 1 mask (all 8‑bit) | Strip buffers: ~ (Nmaps×W×Hstrip) + outstrip | ~ (5–6)×360 KB = 1.8–2.2 MB | ~0.1–0.4 s | 1‑bit partial can be sub‑second on good panels; greyscale slower citeturn1search0turn9search14turn12view0 | Best balance: strong art direction and controllable fog/edge/ink density |
| Relightable compositor | Basic set + normal (2×8‑bit) | Strip buffers (+ extra for Sobel if used) | +720 KB (normal) ⇒ ~2.5–3.0 MB | ~0.2–0.8 s | Still dominated by panel | Only if you truly need changing light direction or stroke orientation |
| Multi-layer scene compositing | Per layer: albedo/light/AO + opacity (several layers) | Strip + per-layer compositing | Multiplies quickly (4–10+ MB) | ~0.5–2+ s | Panel + ghosting mgmt | Only with PSRAM + microSD and a UI reason (layer reveal, transitions) |

**Display update time is usually the hard wall.** For example, a Waveshare 7.5" SPI module lists ~4 s full refresh and ~0.4 s partial refresh, with ~2.1 s for four‑level greyscale refresh. citeturn9search14 In contrast, the 600×600 Inkplate example claims ~0.18 s partial refresh in 1‑bit mode and <1 s full refresh. citeturn1search0turn2search17 The practical implication: optimise the pipeline, yes—but you will still be gating on waveform physics.

### Recommended asset design and pre-bake pipeline

The recommended workflow is aligned with common NPR practice: do geometry and heavy stylisation offline, do lightweight compositing on-device. citeturn10search3turn23search1turn20view0

**Bake offline (strong recommendation):**
- Lightmap / baked lighting (including “global” mood, indirect shading)
- AO
- Curvature/edge maps (and edge thickness variants for art direction)
- Depth (8‑bit is usually sufficient for fog bands on e‑paper; 16‑bit only if you see unpleasant banding after your tone curve)
- Region masks (sky, ground, hero objects, UI‑safe regions)
- Optional: “stroke direction” map (2‑bit or 8‑bit quantised orientations) if you want brush marks aligned to form without normals

**Compute on-device (reasonable):**
- Tone curve (LUT)
- Fog blend from depth (LUT)
- Edge darkening from edge map
- Ordered dithering / quantisation to the display’s grey levels
- Very lightweight paper grain / brush texture modulation

**Compute on-device only if you have PSRAM headroom and it’s worth it:**
- Sobel outlines from depth/normal
- A single separable blur pass as “micro‑bleed” diffusion approximation

### Recommended resolutions and bit depths

Because the final output is 1–4 bpp and e‑paper has limited micro‑contrast, you can downsample *some* maps without obvious loss:

- **Edge/curvature and depth:** keep at full 600×600 (they drive structure).  
- **Albedo/light/AO:** 600×600 if storage is fine; otherwise 300×300 with bilinear upsample often looks acceptable after dithering (test it).  
- **Normal map:** if used only for broad relighting, 300×300 is often enough.

### Storage layout and compression

For ESP32, decoding cost and access pattern matter as much as compression ratio.

1) **Tile/strip chunking is more important than fancy codecs.** Store assets in strips (e.g., 600×32 rows) so a single read fetches contiguous data for each map.

2) **Keep decoding trivial.** For masks and edge maps, RLE often works well. For tonal maps, light compression may not be worth the CPU unless you are flash‑constrained.

3) **Memory-map where possible.** With ESP‑IDF you can map a partition and read it as memory (good for raw packed strips). citeturn13search0

A practical bundle format (conceptual) is:
- Header: width, height, strip height, map channel list, quantisation hints
- Strip directory: offsets of each strip for each channel
- Data: channel blocks stored strip‑major (so you can read `albedo_strip`, `light_strip`, … in one sequential sweep)

## Example offline tooling and map generation workflow

You asked specifically for common content‑creation tools; the key point is that they can generate the maps you need, but you should tailor outputs to e‑paper’s tonal constraints.

- **Blender:** The Cycles bake system supports baking textures such as base colour and normal maps, and baking AO/procedural textures for export. citeturn15search5
- **xatlas:** `xatlas` is explicitly designed to generate unique texture coordinates suitable for baking lightmaps or texture painting—useful if you need texture‑space baking before producing screen‑space passes. citeturn15search0
- **Marmoset Toolbag:** Toolbag’s documentation emphasises baking common map types including normal, AO, curvature, height, and more—curvature/height are directly useful for ink edge pooling and wash control masks. citeturn15search4turn15search7

**File formats for ESP32 consumption (practical guidance):**
- Prefer simple raw/packed formats (custom `.bin`) over PNG/JPEG if the goal is deterministic CPU time and low RAM.
- If you must use a standard format, choose something with low decode complexity (e.g., raw + RLE) and decode stripwise.
- If you need a user‑editable pipeline, keep “master” assets as PNG/TIFF offline, then convert to the device bundle via a custom packer that quantises to 8‑bit (or 4‑bit for masks) and writes strip blocks.

## Risks, limitations, alternatives, and validation benchmarks

### Key implementation risks

**Greyscale + partial update is not a given.** Many controllers treat “fast/partial” and “high‑quality greyscale” as different modes, with fast modes commonly restricted to black/white (A2), and greyscale modes (GC16) being slower. citeturn12view0turn11view0turn9search14 If your panel/controller does not expose a usable 4‑bpp update path, you will end up relying on 1‑bit dithering for speed.

**Ghosting management is mandatory.** Vendors explicitly warn against endless partial updates without periodic full refresh. citeturn9search2turn11view0turn9search8 Your pipeline must include a “maintenance refresh” schedule, and your UX must tolerate it (flicker, time).

**Documentation inconsistencies exist in the wild.** Even vendor/product ecosystems sometimes contain conflicting panel specs across pages (resolution/feature tables). That means: treat your specific panel/controller datasheet as the source of truth, and prototype on the exact hardware you will ship.

**PSRAM throughput and cache behaviour can bite you.** External RAM is useful, but ESP‑IDF documents cache coupling with flash and inaccessibility when flash cache is disabled. citeturn14view0 Over‑aggressive use of PSRAM for frequently accessed hot data can reduce performance rather than improve it.

### Alternative approaches if the map compositor becomes too heavy

**Vectorised stroke rendering (procedural strokes):**  
Instead of storing multiple raster maps, store a compact stroke list (polylines + width + “ink amount”). Many NPR systems describe stroke‑based approaches, but they usually assume GPU acceleration; on ESP32 you’d need a very constrained stroke model (few hundred strokes) to keep rasterisation cheap. citeturn23search1turn20view0 The advantage is tiny storage and natural “brush” character; the downside is limited scene complexity and difficult art direction.

**Pre-rendered frames / keyframes:**  
If the camera/view is fixed, pre-render stylised output offline and store as 1–4 bpp frames. This is the most robust approach and avoids on-device NPR complexity. The trade is storage vs flexibility.

**Multi-pass partial refresh tricks:**  
If the controller supports window updates, you can update UI elements (text/time) in 1‑bit fast mode while leaving the art background static, then periodically recompute and do a full greyscale refresh. This pattern is broadly consistent with “partial refresh for dynamic elements; full refresh for quality/cleanup”. citeturn11view0turn9search2

### Experiments and benchmarks to validate on real hardware

A credible feasibility conclusion requires running these on the target board and panel:

**Throughput and buffering**
- Measure time to push a full 600×600 frame at 1 bpp vs 4 bpp over your actual bus (SPI vs parallel/I80).
- Verify whether DMA requires internal buffers; if yes, quantify the largest strip buffer you can allocate without heap fragmentation. citeturn13search16turn14view0

**Compute microbenchmarks**
- Implement three pipelines: (A) tone+fog only, (B) +edge map, (C) +stroke texture + dithering. Time each with cycle counters.
- Measure with and without PSRAM for working buffers, and with Wi‑Fi enabled vs disabled (contention and cache effects show up quickly in practice on ESP32‑class MCUs). citeturn14view0

**Display mode validation**
- Confirm which modes your panel actually supports: 1‑bit fast/partial (A2‑like), 4‑bit GC16‑like, and how partial windows behave in each.
- Characterise ghosting by running 50–200 partial updates and observing degradation; determine safe “N partial updates then full refresh” for your artwork style. Vendor guidance indicates you must do periodic full refresh. citeturn9search2turn11view0turn9search8

**Quality metrics for ink‑wash look**
- Compare: (i) native greyscale (if supported) vs (ii) 1‑bit ordered dithering vs (iii) error diffusion. On e‑paper, ordered dither often looks less “noisy” at distance, while error diffusion looks more photographic but can shimmer/texture in unpleasant ways for ink wash.
- Evaluate whether downsampling albedo/light/AO to 300×300 is visually acceptable after dithering; many scenes are, because structural cues (edges/depth) dominate perception on e‑paper.

**Power**
- Measure current draw during refresh and deep sleep for your board; e‑paper’s key advantage is near‑zero power to retain an image, but refresh energy dominates if you update often. Waveshare documentation stresses “power is basically only required for refreshing” and recommends sleeping/power‑off between updates. citeturn11view0turn1search0

### Final feasibility verdict

For a 600×600 monochrome/greyscale e‑paper target, an ESP32 can absolutely produce compelling sumi‑e stylisation **if you engineer the content pipeline around the display’s waveform constraints** and treat the MCU as a **tile‑based compositor of pre‑baked passes**, not as a real‑time 3D renderer. The highest‑leverage pre‑bakes are lightmaps, AO, depth, and stable edge/curvature maps; the highest‑leverage on-device operations are LUT tone curves, depth fog blending, and an efficient dithering strategy tuned to your panel’s greyscale mode. citeturn10search3turn20view0turn12view0turn0search4