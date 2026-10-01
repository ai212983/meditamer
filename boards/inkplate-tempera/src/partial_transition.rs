// Called only from the hardware-gated display path (or, below, from this
// module's own tests): dead code on a host build with that path excluded.
#![cfg_attr(not(any(test, target_os = "none")), allow(dead_code))]

#[cfg(test)]
#[path = "partial_transition/tests.rs"]
mod tests;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PartialRowSpan {
    pub(crate) scan_rows: usize,
    pub(crate) first_changed_row: usize,
    pub(crate) last_changed_row: usize,
    pub(crate) changed_span_rows: usize,
    pub(crate) source_skip_candidate_rows: usize,
}

impl PartialRowSpan {
    /// Grows `scan_rows` by up to `guard_rows` additional rows driven with
    /// real transition data below `first_changed_row`, moving the scan's
    /// real/neutral gate-drain seam clear of the actual first-changed row
    /// rather than leaving it sitting exactly on it. Never exceeds the full
    /// frame. `first_changed_row`, `last_changed_row`, and
    /// `changed_span_rows` keep describing the true changed span -- only the
    /// driven amount grows.
    pub(crate) fn widened_by(mut self, guard_rows: usize) -> Self {
        let row_count = self.scan_rows + self.first_changed_row;
        self.scan_rows = self.scan_rows.saturating_add(guard_rows).min(row_count);
        self
    }
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

/// Describes the changed framebuffer-row span and the work performed by the
/// current reverse-ordered panel scan. The scan starts at the last framebuffer
/// row, so `source_skip_candidate_rows` counts unchanged rows it currently
/// source-clocks before reaching the last changed row.
pub(crate) fn reverse_scan_row_span_for_changes(
    previous: &[u8],
    current: &[u8],
    row_bytes: usize,
) -> Option<PartialRowSpan> {
    assert_eq!(previous.len(), current.len());
    assert!(row_bytes != 0 && previous.len().is_multiple_of(row_bytes));

    // Grouping four byte comparisons per iteration is the measured panel
    // optimum for this PSRAM-backed search. Byte-wise and eight-byte grouping
    // were slower for sparse spans, while row-wise ROM memcmp also regressed.
    // Keep the exact byte result so row semantics and the no-change case stay
    // unchanged without allocating an intermediate buffer.
    let first_changed_byte = first_changed_byte_chunk4(previous, current)?;
    let last_changed_byte = last_changed_byte_chunk4(previous, current)
        .expect("a first changed byte implies a last changed byte");
    let row_count = previous.len() / row_bytes;
    let first_changed_row = first_changed_byte / row_bytes;
    let last_changed_row = last_changed_byte / row_bytes;
    Some(PartialRowSpan {
        scan_rows: row_count - first_changed_row,
        first_changed_row,
        last_changed_row,
        changed_span_rows: last_changed_row - first_changed_row + 1,
        source_skip_candidate_rows: row_count - last_changed_row - 1,
    })
}

#[inline(always)]
fn first_changed_byte_chunk4(previous: &[u8], current: &[u8]) -> Option<usize> {
    let mut index = 0usize;
    while index + 4 <= previous.len() {
        let changed = (previous[index] ^ current[index])
            | (previous[index + 1] ^ current[index + 1])
            | (previous[index + 2] ^ current[index + 2])
            | (previous[index + 3] ^ current[index + 3]);
        if changed != 0 {
            for offset in 0..4 {
                if previous[index + offset] != current[index + offset] {
                    return Some(index + offset);
                }
            }
        }
        index += 4;
    }
    while index < previous.len() {
        if previous[index] != current[index] {
            return Some(index);
        }
        index += 1;
    }
    None
}

#[inline(always)]
fn last_changed_byte_chunk4(previous: &[u8], current: &[u8]) -> Option<usize> {
    let mut index = previous.len();
    while index >= 4 {
        let base = index - 4;
        let changed = (previous[base] ^ current[base])
            | (previous[base + 1] ^ current[base + 1])
            | (previous[base + 2] ^ current[base + 2])
            | (previous[base + 3] ^ current[base + 3]);
        if changed != 0 {
            for offset in (0..4).rev() {
                if previous[base + offset] != current[base + offset] {
                    return Some(base + offset);
                }
            }
        }
        index = base;
    }
    while index != 0 {
        index -= 1;
        if previous[index] != current[index] {
            return Some(index);
        }
    }
    None
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
    prepare_partial_transition_inner(
        previous,
        current,
        transition,
        previous.len(),
        lut_white,
        lut_black,
    );
}

/// Builds only the transition-buffer suffix consumed by a reverse scan of the
/// final `source_bytes` framebuffer bytes. The untouched prefix is never read
/// by that scan and deliberately retains its previous contents.
pub(crate) fn prepare_partial_transition_suffix(
    previous: &[u8],
    current: &[u8],
    transition: &mut [u8],
    source_bytes: usize,
    lut_white: &[u8; 16],
    lut_black: &[u8; 16],
) {
    if source_bytes == previous.len() {
        prepare_partial_transition(previous, current, transition, lut_white, lut_black);
        return;
    }
    prepare_partial_transition_inner(
        previous,
        current,
        transition,
        source_bytes,
        lut_white,
        lut_black,
    );
}

#[inline(always)]
fn prepare_partial_transition_inner(
    previous: &[u8],
    current: &[u8],
    transition: &mut [u8],
    source_bytes: usize,
    lut_white: &[u8; 16],
    lut_black: &[u8; 16],
) {
    assert_eq!(previous.len(), current.len());
    assert_eq!(transition.len(), previous.len() * 2);
    assert!(source_bytes <= previous.len());

    let mut source = previous.len();
    let mut destination = transition.len();
    let source_start = source - source_bytes;
    while source != source_start {
        source -= 1;
        let previous = previous[source];
        let current = current[source];
        let lower = partial_waveform_byte(previous, current, false, lut_white, lut_black);
        let upper = partial_waveform_byte(previous, current, true, lut_white, lut_black);
        destination -= 1;
        transition[destination] = upper;
        destination -= 1;
        transition[destination] = lower;
    }
    debug_assert_eq!(destination, source_start * 2);
}
