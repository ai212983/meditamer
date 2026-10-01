# proc-macro-error2 future-compatibility patch

Status: repository-owned compatibility patch

## Immutable base

- Package: `proc-macro-error2` 2.0.1
- Crates.io checksum: `11ec05c5fb91670a955f9eb8d0e48d8a992b2b7680807d89e80f62a4a6bc8b18`
- License: MIT OR Apache-2.0
- Patched crate-tree SHA-256 (excluding this manifest):
  `27a39808937bcc4cf5f12951c58ae0e60b595c7988cb72c8bb4b3063e7c2a5ae`

## Maintained delta

Rust issue 127909 is phasing out re-exporting a private `extern crate` item.
The crate's hidden macro support module publicly re-exports `proc_macro`, so the
root declaration is changed from `extern crate proc_macro` to
`pub extern crate proc_macro`, exactly matching rustc's compatibility guidance.

Remove this patch when an upstream release contains the same fix.
