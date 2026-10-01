# mountain-snow

Runtime crate for the Ambient Home mountain: a
PMV-to-percentage policy plus a streaming one-bit composer and
the SD pack codec. `no_std`, no heap, no statics, one dependency (the
`asset-source` access contract, itself dependency-free); host
and device agree bit for bit. All retained buffers and source rows stay
with the caller — on the device, fallibly in PSRAM and on SD.

- `pmv` is the Fanger heat-balance calculation in SI units (ISO 7730:2025
  model, ASHRAE 55 Appendix B algorithm, cross-checked against
  `pythermalcomfort`), plus seasonal clothing insulation and the
  PMV-to-snow display convention. Device assumptions (`tr = ta`,
  `vr = 0.1`, `met = 1.0`, `wme = 0.0`) are explicit constants.
- `policy` maps one parent-tick PMV sample to a snow percentage
  with hysteresis (0.05 PMV) and a minimum visible step (2 points). It requests
  no refresh of its own: `None` means nothing visible changed.
- `composer` resolves one caller-owned row (rock/snow gray, barrier,
  eligibility bits, blue-noise) to packed one-bit pixels through a
  barrier-level cut. Dither is host-exact by construction.
- `pack` parses the v1 SD pack (64-byte header, IEEE CRC32, row bands)
  with borrowed row accessors, plus the `Header::file_len`,
  `payload_range`, and `row_ranges` primitives and the incremental `Crc32`
  digest a streaming loader needs. Built by
  `tools/mountain_snow/build_pack.py`.
- `source` validates a pack image held by any `asset-source` `AssetSource`
  (`validate_source` reads exactly the header, requires the source length
  to equal `Header::file_len`, then streams the payload once through a
  caller-owned CRC scratch buffer) and publishes the validated immutable
  header as a `ValidatedPack`. `ValidatedPack::read_row` fills a
  caller-owned `RowScratch` (2475 bytes: rock, snow, barrier, eligibility,
  noise planes) with exactly five range reads, re-checking the source
  length first so a source whose length changed cannot reuse stale metadata.
  `ValidatedPack` is not bound to a source identity or generation: the
  recheck catches length changes only, not same-length content replacement,
  so the caller must keep the same source bytes immutable between validation
  and row reads.
  `RowScratch::inputs` borrows the planes for `composer::compose_row`.
  The firmware SD read path (`products/meditamer/src/firmware/storage/sd_task/mountain_read.rs`)
  uses this streaming shape: one `MountainSession` per upload generation
  (validated header plus histogram), five range reads per stored row into
  one `RowScratch`, and `compose_row_for_percent` into a 40,350-byte
  overlay. Host tests prove the byte-identical composition; device timing
  and power evidence remain open, so no device qualification is claimed.

Host tests, strict Clippy, and coverage run through the `mountain-snow`
row in `scripts/host-suites.tsv`:

```sh
bash scripts/host-test.sh test mountain-snow
bash scripts/host-test.sh lint mountain-snow
```
