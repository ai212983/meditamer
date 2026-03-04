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
