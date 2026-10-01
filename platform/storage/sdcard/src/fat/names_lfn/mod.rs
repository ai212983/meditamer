//! Name handling: path splitting, short-name matching, and long-file-name decoding.

mod lfn_decode;
mod path_display;

pub(super) use lfn_decode::{
    build_display_name, build_display_name_into, consume_lfn_entry, short_name_checksum,
};
pub(super) use path_display::{
    parse_path, parse_record, path_segment_to_name, segment_matches_record,
};
