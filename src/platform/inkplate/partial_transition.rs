#[inline(always)]
pub(crate) fn partial_waveform_byte(
    previous: u8,
    current: u8,
    upper_nibble: bool,
    lut_white: &[u8; 16],
    lut_black: &[u8; 16],
) -> u8 {
    let black_to_white = previous & !current;
    let white_to_black = !previous & current;
    let shift = if upper_nibble { 4 } else { 0 };
    lut_white[((black_to_white >> shift) & 0x0F) as usize]
        & lut_black[((white_to_black >> shift) & 0x0F) as usize]
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PartialTransitionStats {
    pub(crate) changed_bytes: usize,
    pub(crate) changed_pixels: u32,
}

pub(crate) fn partial_transition_stats(previous: &[u8], current: &[u8]) -> PartialTransitionStats {
    assert_eq!(previous.len(), current.len());

    let mut stats = PartialTransitionStats::default();
    for (&previous, &current) in previous.iter().zip(current) {
        let changed = previous ^ current;
        if changed != 0 {
            stats.changed_bytes += 1;
            stats.changed_pixels += changed.count_ones();
        }
    }
    stats
}

/// Returns the number of gate rows a reverse-ordered panel scan must emit to
/// include every changed framebuffer byte. The scan always starts at the last
/// framebuffer row, so only unchanged trailing rows after the final required
/// gate row can be omitted.
pub(crate) fn reverse_scan_row_count_for_changes(
    previous: &[u8],
    current: &[u8],
    row_bytes: usize,
) -> Option<usize> {
    assert_eq!(previous.len(), current.len());
    assert!(row_bytes != 0 && previous.len().is_multiple_of(row_bytes));

    let first_changed_byte = previous
        .iter()
        .zip(current)
        .position(|(previous, current)| previous != current)?;
    let first_changed_row = first_changed_byte / row_bytes;
    Some(previous.len() / row_bytes - first_changed_row)
}

/// Builds the exact reverse-ordered transition frame consumed by the
/// Inkplate 4 TEMPERA reference driver's partial scan loop.
pub(crate) fn prepare_partial_transition(
    previous: &[u8],
    current: &[u8],
    transition: &mut [u8],
    lut_white: &[u8; 16],
    lut_black: &[u8; 16],
) {
    assert_eq!(previous.len(), current.len());
    assert_eq!(transition.len(), previous.len() * 2);

    let mut source = previous.len();
    let mut destination = transition.len();
    while source != 0 {
        source -= 1;
        destination -= 1;
        transition[destination] = partial_waveform_byte(
            previous[source],
            current[source],
            true,
            lut_white,
            lut_black,
        );
        destination -= 1;
        transition[destination] = partial_waveform_byte(
            previous[source],
            current[source],
            false,
            lut_white,
            lut_black,
        );
    }
    debug_assert_eq!(destination, 0);
}
