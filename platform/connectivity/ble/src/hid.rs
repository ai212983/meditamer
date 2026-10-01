//! Bounded HID descriptor discovery and model-neutral report decoding.
//!
//! Tested with synthetic descriptors and a captured GamepadSpace-Q37 map.
//! Analog values follow descriptor field order, with their meanings in
//! `ReportLayout::axes`; they are not fixed X/Y/Z/Rx/Ry/Rz slots.
//!
//! Supports separate top-level collections with nested collections: `Usage Page`/
//! `Usage`/`Usage Minimum`/`Usage Maximum` (local, cleared after every Main
//! item per the HID spec), `Report Size`/`Report Count`/`Report ID`/
//! `Logical Minimum`/`Logical Maximum` (global, persisted until changed),
//! and `Input`/`Collection`/`End Collection` (main items). `Push`/`Pop` are
//! explicitly unsupported (rejected, not silently mis-decoded) -- gamepad
//! descriptors essentially never need them for the top button/axis/hat
//! fields this module extracts, and getting that wrong would corrupt every
//! field after it.

use heapless::Vec;

use crate::capacity::{
    GAMEPAD_AXES_MAX, GAMEPAD_BUTTONS_MAX, HID_DESCRIPTOR_MAX_BYTES, HID_REPORTS_MAX,
    HID_REPORT_MAX_BYTES,
};

const USAGE_PAGE_GENERIC_DESKTOP: u32 = 0x01;
const USAGE_PAGE_BUTTON: u32 = 0x09;
const USAGE_HAT_SWITCH: u32 = 0x39;

/// Recognized HID analog usages. The page is part of the identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisUsage {
    X,
    Y,
    Z,
    Rx,
    Ry,
    Rz,
    Accelerator,
    Brake,
}

impl AxisUsage {
    fn from_hid(page: u32, usage: u32) -> Option<Self> {
        Some(match (page, usage) {
            (1, 0x30) => Self::X,
            (1, 0x31) => Self::Y,
            (1, 0x32) => Self::Z,
            (1, 0x33) => Self::Rx,
            (1, 0x34) => Self::Ry,
            (1, 0x35) => Self::Rz,
            (2, 0xc4) => Self::Accelerator,
            (2, 0xc5) => Self::Brake,
            _ => return None,
        })
    }
}

/// Why descriptor discovery rejected a descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorError {
    /// The descriptor does not fit [`crate::capacity::HID_DESCRIPTOR_MAX_BYTES`].
    TooLarge,
    /// A short item's prefix claimed more data bytes than remain.
    Truncated,
    /// `Push`/`Pop` appeared; unsupported (see this module's doc).
    UnsupportedPushPop,
    /// More button usages than [`crate::capacity::GAMEPAD_BUTTONS_MAX`], or
    /// more axis fields than [`crate::capacity::GAMEPAD_AXES_MAX`].
    TooManyFields,
    /// More top-level report layouts than [`crate::capacity::HID_REPORTS_MAX`].
    TooManyReports,
    /// An `Input` item appeared with no `Report Size`/`Report Count` set.
    MissingReportShape,
    /// No top-level `Application` collection was found at all.
    NoApplicationCollection,
}

/// One decoded analog axis field's position and legal range within a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisField {
    pub usage: AxisUsage,
    pub bit_offset: u16,
    pub bit_size: u8,
    pub logical_min: i32,
    pub logical_max: i32,
}

/// The bit layout `HidDescriptor::parse` extracted from one report: where
/// the button bitmask, hat switch, and each axis live, and this report's ID
/// (if the descriptor declared one -- "multiple reports" means more than
/// one distinct `ReportLayout`, disambiguated by `report_id`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportLayout {
    pub report_id: Option<u8>,
    pub button_bit_offset: Option<u16>,
    pub button_count: u8,
    pub hat_bit_offset: Option<u16>,
    pub hat_bit_size: u8,
    pub hat_has_null: bool,
    pub hat_logical_min: i32,
    pub hat_logical_max: i32,
    /// Fields in descriptor order, matching [`DecodedReport::axes`] indices.
    pub axes: Vec<AxisField, GAMEPAD_AXES_MAX>,
    total_bits: u16,
}

impl ReportLayout {
    /// Total report length in bytes, including the leading report-ID byte
    /// when this layout has one.
    pub fn byte_len(&self) -> usize {
        let id_byte = usize::from(self.report_id.is_some());
        id_byte + usize::from(self.total_bits.div_ceil(8))
    }
}

/// A discovered, bounded set of report layouts (plan: "multiple reports").
#[derive(Debug)]
pub struct HidDescriptor {
    layouts: Vec<ReportLayout, HID_REPORTS_MAX>,
}

impl HidDescriptor {
    /// Parse a raw HID report descriptor into its bounded, decoded report
    /// layouts. See this module's doc for exactly what item grammar is
    /// supported.
    pub fn parse(bytes: &[u8]) -> Result<Self, DescriptorError> {
        if bytes.len() > HID_DESCRIPTOR_MAX_BYTES {
            return Err(DescriptorError::TooLarge);
        }

        let mut walker = Walker::new(bytes);
        let mut layouts: Vec<ReportLayout, HID_REPORTS_MAX> = Vec::new();
        let mut state = ParseState::default();

        while let Some(item) = walker.next_item()? {
            let is_main_item = item.is_main();
            state.apply_item(item, &mut layouts)?;
            if is_main_item {
                state.clear_local_items();
            }
        }

        if !state.seen_application_collection {
            return Err(DescriptorError::NoApplicationCollection);
        }

        Ok(Self { layouts })
    }

    /// The discovered report layouts, in descriptor order.
    pub fn layouts(&self) -> &[ReportLayout] {
        &self.layouts
    }

    /// The layout matching `report_id` (or the sole layout, if the
    /// descriptor declared no report IDs at all and `report_id` is `None`).
    pub fn layout_for(&self, report_id: Option<u8>) -> Option<&ReportLayout> {
        self.layouts
            .iter()
            .find(|layout| layout.report_id == report_id)
    }
}

#[derive(Default)]
struct ParseState {
    // Global state, persists across Main items until changed.
    usage_page: u32,
    usages: Vec<u32, GAMEPAD_BUTTONS_MAX>,
    usage_minimum: Option<u32>,
    usage_maximum: Option<u32>,
    report_size: Option<u32>,
    report_count: Option<u32>,
    report_id: Option<u8>,
    logical_min: i32,
    logical_max: i32,

    collection_depth: u32,
    bit_cursor: u16,
    seen_application_collection: bool,
    current: Option<ReportLayout>,
}

impl ParseState {
    fn apply_item(
        &mut self,
        item: Item,
        layouts: &mut Vec<ReportLayout, HID_REPORTS_MAX>,
    ) -> Result<(), DescriptorError> {
        match item {
            Item::UsagePage(value) => self.usage_page = value,
            Item::Usage(value) => {
                self.usages
                    .push(value)
                    .map_err(|_| DescriptorError::TooManyFields)?;
            }
            Item::UsageMinimum(value) => self.usage_minimum = Some(value),
            Item::UsageMaximum(value) => self.usage_maximum = Some(value),
            Item::ReportSize(value) => self.report_size = Some(value),
            Item::ReportCount(value) => self.report_count = Some(value),
            Item::ReportId(value) => {
                self.report_id = Some(value);
                if let Some(layout) = self.current.as_mut() {
                    layout.report_id = self.report_id;
                }
            }
            Item::LogicalMinimum(value) => self.logical_min = value,
            Item::LogicalMaximum(value) => self.logical_max = value,
            Item::PushPop => return Err(DescriptorError::UnsupportedPushPop),
            Item::Collection => self.open_collection(),
            Item::EndCollection => self.close_collection(layouts)?,
            Item::Input {
                constant,
                null_state,
            } => self.apply_input(constant, null_state)?,
            Item::OtherMain => {}
        }
        Ok(())
    }

    fn apply_input(&mut self, constant: bool, null_state: bool) -> Result<(), DescriptorError> {
        let size = self
            .report_size
            .ok_or(DescriptorError::MissingReportShape)?;
        let count = self
            .report_count
            .ok_or(DescriptorError::MissingReportShape)?;
        let field_bits = size.saturating_mul(count) as u16;
        self.current
            .as_mut()
            .ok_or(DescriptorError::NoApplicationCollection)?;

        if !constant {
            self.apply_non_constant_input(size, count, null_state)?;
        }

        self.bit_cursor = self.bit_cursor.saturating_add(field_bits);
        Ok(())
    }

    fn apply_non_constant_input(
        &mut self,
        size: u32,
        count: u32,
        null_state: bool,
    ) -> Result<(), DescriptorError> {
        if self.usage_page == USAGE_PAGE_BUTTON
            && self.usage_minimum.is_some()
            && self.usage_maximum.is_some()
        {
            if let Some(layout) = self.current.as_mut() {
                layout.button_bit_offset.get_or_insert(self.bit_cursor);
                layout.button_count = layout
                    .button_count
                    .saturating_add(count.min(u32::from(GAMEPAD_BUTTONS_MAX as u8)) as u8);
            }
            return Ok(());
        }

        for index in 0..self.usages.len().min(count as usize) {
            let usage = self.usages[index];
            let field_offset = self.bit_cursor + (index as u16) * (size as u16);
            self.apply_axis_or_hat_input_field(usage, field_offset, size as u8, null_state)?;
        }

        Ok(())
    }

    fn apply_axis_or_hat_input_field(
        &mut self,
        usage: u32,
        field_offset: u16,
        bit_size: u8,
        null_state: bool,
    ) -> Result<(), DescriptorError> {
        if self.usage_page == USAGE_PAGE_GENERIC_DESKTOP && usage == USAGE_HAT_SWITCH {
            if let Some(layout) = self.current.as_mut() {
                layout.hat_bit_offset = Some(field_offset);
                layout.hat_bit_size = bit_size;
                layout.hat_has_null = null_state;
                layout.hat_logical_min = self.logical_min;
                layout.hat_logical_max = self.logical_max;
            }
            return Ok(());
        }

        if let Some(axis_usage) = AxisUsage::from_hid(self.usage_page, usage) {
            let layout = self
                .current
                .as_mut()
                .ok_or(DescriptorError::NoApplicationCollection)?;
            layout
                .axes
                .push(AxisField {
                    usage: axis_usage,
                    bit_offset: field_offset,
                    bit_size,
                    logical_min: self.logical_min,
                    logical_max: self.logical_max,
                })
                .map_err(|_| DescriptorError::TooManyFields)?;
        }

        Ok(())
    }

    fn clear_local_items(&mut self) {
        self.usages.clear();
        self.usage_minimum = None;
        self.usage_maximum = None;
    }

    fn open_collection(&mut self) {
        if self.collection_depth == 0 {
            self.seen_application_collection = true;
            let layout = self.current.get_or_insert(ReportLayout {
                report_id: self.report_id,
                button_bit_offset: None,
                button_count: 0,
                hat_bit_offset: None,
                hat_bit_size: 0,
                hat_has_null: false,
                hat_logical_min: 0,
                hat_logical_max: 0,
                axes: Vec::new(),
                total_bits: 0,
            });
            layout.report_id = self.report_id;
            self.bit_cursor = 0;
        }
        self.collection_depth += 1;
    }

    fn close_collection(
        &mut self,
        layouts: &mut Vec<ReportLayout, HID_REPORTS_MAX>,
    ) -> Result<(), DescriptorError> {
        self.collection_depth = self.collection_depth.saturating_sub(1);
        if self.collection_depth == 0 {
            if let Some(mut layout) = self.current.take() {
                layout.total_bits = self.bit_cursor;
                layouts
                    .push(layout)
                    .map_err(|_| DescriptorError::TooManyReports)?;
            }
        }
        Ok(())
    }
}

/// Why [`decode_report`] rejected a report (plan: "malformed input" is a
/// distinguishable, safe outcome, never a corrupted or partial sample).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The raw bytes are longer than [`crate::capacity::HID_REPORT_MAX_BYTES`].
    TooLarge,
    /// The raw bytes are shorter than `layout.byte_len()` requires.
    Truncated,
    /// The layout's leading report-ID byte does not match the raw bytes'.
    ReportIdMismatch,
}

/// One decoded gamepad-shaped sample: raw bit-extracted field values, no
/// scaling or product-specific interpretation (that is
/// [`crate::input::GamepadSample`]'s job, once a product maps these axes to
/// its own controls).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedReport {
    pub buttons: u16,
    pub hat: Option<i32>,
    /// Raw values in [`ReportLayout::axes`] order; unused slots are `None`.
    pub axes: [Option<i32>; GAMEPAD_AXES_MAX],
}

/// Extract a `bit_size`-wide field at `bit_offset` from `bytes`, LSB-first
/// within each byte and byte-order ascending -- USB HID report bit
/// numbering.
fn extract_bits(bytes: &[u8], bit_offset: u16, bit_size: u8) -> u32 {
    let mut value: u32 = 0;
    for bit in 0..u16::from(bit_size) {
        let absolute_bit = bit_offset + bit;
        let byte_index = (absolute_bit / 8) as usize;
        let bit_index = absolute_bit % 8;
        let byte = bytes.get(byte_index).copied().unwrap_or(0);
        let bit_value = (byte >> bit_index) & 1;
        value |= u32::from(bit_value) << bit;
    }
    value
}

fn sign_extend(raw: u32, bit_size: u8, logical_min: i32) -> i32 {
    if logical_min >= 0 || bit_size == 0 || bit_size >= 32 {
        return raw as i32;
    }
    let sign_bit = 1u32 << (bit_size - 1);
    if raw & sign_bit != 0 {
        (raw | !((sign_bit << 1).wrapping_sub(1))) as i32
    } else {
        raw as i32
    }
}

/// Decode one raw report against `layout`. `HID_REPORT_MAX_BYTES` bounds
/// `bytes`; a layout wider than that can never be built from a descriptor
/// that itself fit [`crate::capacity::HID_DESCRIPTOR_MAX_BYTES`] for any
/// gamepad-shaped report, so this only rejects malformed input, never a
/// legal layout.
pub fn decode_report(layout: &ReportLayout, bytes: &[u8]) -> Result<DecodedReport, DecodeError> {
    if bytes.len() > HID_REPORT_MAX_BYTES {
        return Err(DecodeError::TooLarge);
    }
    if bytes.len() < layout.byte_len() {
        return Err(DecodeError::Truncated);
    }

    let (report_id_byte, field_bytes) = match layout.report_id {
        Some(_) => {
            let (id, rest) = bytes.split_first().ok_or(DecodeError::Truncated)?;
            (Some(*id), rest)
        }
        None => (None, bytes),
    };
    if let (Some(expected), Some(actual)) = (layout.report_id, report_id_byte) {
        if expected != actual {
            return Err(DecodeError::ReportIdMismatch);
        }
    }

    let buttons = match layout.button_bit_offset {
        Some(offset) => extract_bits(field_bytes, offset, layout.button_count.min(16)) as u16,
        None => 0,
    };

    let hat = layout.hat_bit_offset.and_then(|offset| {
        let raw = extract_bits(field_bytes, offset, layout.hat_bit_size);
        let value = sign_extend(raw, layout.hat_bit_size, layout.hat_logical_min);
        if layout.hat_has_null
            && !(layout.hat_logical_min..=layout.hat_logical_max).contains(&value)
        {
            None
        } else {
            Some(value)
        }
    });

    let mut axes: [Option<i32>; GAMEPAD_AXES_MAX] = [None; GAMEPAD_AXES_MAX];
    for (slot, axis) in layout.axes.iter().enumerate() {
        let raw = extract_bits(field_bytes, axis.bit_offset, axis.bit_size);
        axes[slot] = Some(sign_extend(raw, axis.bit_size, axis.logical_min));
    }

    Ok(DecodedReport { buttons, hat, axes })
}

/// A neutral (all-buttons-released, all-axes-centered, hat-in-null-state)
/// report, for disconnect or decode failure. Sticks use their logical
/// midpoint; accelerator/brake use their logical minimum. This is a fallback,
/// not a measurement of the controller's physical resting values.
pub fn neutral_report(layout: &ReportLayout) -> DecodedReport {
    let mut axes: [Option<i32>; GAMEPAD_AXES_MAX] = [None; GAMEPAD_AXES_MAX];
    for (slot, axis) in layout.axes.iter().enumerate() {
        axes[slot] = Some(match axis.usage {
            AxisUsage::Accelerator | AxisUsage::Brake => axis.logical_min,
            _ => {
                (i64::from(axis.logical_min)
                    + (i64::from(axis.logical_max) - i64::from(axis.logical_min) + 1) / 2)
                    as i32
            }
        });
    }
    DecodedReport {
        buttons: 0,
        hat: None,
        axes,
    }
}

enum Item {
    UsagePage(u32),
    Usage(u32),
    UsageMinimum(u32),
    UsageMaximum(u32),
    ReportSize(u32),
    ReportCount(u32),
    ReportId(u8),
    LogicalMinimum(i32),
    LogicalMaximum(i32),
    PushPop,
    Collection,
    EndCollection,
    Input { constant: bool, null_state: bool },
    OtherMain,
}

impl Item {
    fn is_main(&self) -> bool {
        matches!(
            self,
            Item::Collection | Item::EndCollection | Item::Input { .. } | Item::OtherMain
        )
    }
}

struct Walker<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Walker<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn next_item(&mut self) -> Result<Option<Item>, DescriptorError> {
        loop {
            let Some(&prefix) = self.bytes.get(self.cursor) else {
                return Ok(None);
            };
            self.cursor += 1;

            let size_code = prefix & 0b0000_0011;
            let item_type = (prefix & 0b0000_1100) >> 2;
            let tag = (prefix & 0b1111_0000) >> 4;
            let data_len = match size_code {
                0 => 0,
                1 => 1,
                2 => 2,
                _ => 4,
            };
            if self.cursor + data_len > self.bytes.len() {
                return Err(DescriptorError::Truncated);
            }
            let data = &self.bytes[self.cursor..self.cursor + data_len];
            self.cursor += data_len;

            let unsigned = read_unsigned(data);
            let signed = read_signed(data);

            let item = match (item_type, tag) {
                (1, 0) => Item::UsagePage(unsigned),
                (1, 1) => Item::LogicalMinimum(signed),
                (1, 2) => Item::LogicalMaximum(signed),
                (1, 7) => Item::ReportSize(unsigned),
                (1, 8) => Item::ReportId(unsigned as u8),
                (1, 9) => Item::ReportCount(unsigned),
                (1, 10) | (1, 11) => Item::PushPop,
                (2, 0) => Item::Usage(unsigned),
                (2, 1) => Item::UsageMinimum(unsigned),
                (2, 2) => Item::UsageMaximum(unsigned),
                (0, 8) => Item::Input {
                    constant: unsigned & 0x01 != 0,
                    null_state: unsigned & 0x40 != 0,
                },
                (0, 10) => Item::Collection,
                (0, 12) => Item::EndCollection,
                (0, _) => Item::OtherMain,
                // Every other item (Physical Min/Max,
                // Unit, Unit Exponent, Designator/String/Delimiter
                // locals, ...) is consumed for its bytes and otherwise
                // ignored: it does not change bit layout on its own, and
                // this module only extracts button/hat/axis Input fields.
                _ => continue,
            };
            return Ok(Some(item));
        }
    }
}

fn read_unsigned(data: &[u8]) -> u32 {
    let mut value: u32 = 0;
    for (index, &byte) in data.iter().enumerate() {
        value |= u32::from(byte) << (8 * index);
    }
    value
}

fn read_signed(data: &[u8]) -> i32 {
    match data.len() {
        0 => 0,
        1 => i32::from(data[0] as i8),
        2 => i32::from(i16::from_le_bytes([data[0], data[1]])),
        _ => {
            let mut buf = [0u8; 4];
            buf[..data.len().min(4)].copy_from_slice(&data[..data.len().min(4)]);
            i32::from_le_bytes(buf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Original descriptor from GamepadSpace-Q37, captured through macOS HIDAPI.
    const Q37_DESCRIPTOR: &[u8] = include_bytes!("../tests/fixtures/q37-gamepad-descriptor.bin");

    #[test]
    fn q37_nested_collections_do_not_shift_axes() {
        let descriptor = HidDescriptor::parse(Q37_DESCRIPTOR).unwrap();
        assert_eq!(descriptor.layout_for(Some(3)).unwrap().byte_len(), 3);
        let layout = descriptor.layout_for(Some(4)).unwrap();
        assert_eq!(layout.byte_len(), 11);
        let offsets: Vec<_, GAMEPAD_AXES_MAX> =
            layout.axes.iter().map(|axis| axis.bit_offset).collect();
        assert_eq!(offsets.as_slice(), &[0, 8, 16, 24, 56, 64]);
        assert_eq!(layout.hat_bit_offset, Some(32));
        assert_eq!(layout.button_bit_offset, Some(40));
        let usages: Vec<_, GAMEPAD_AXES_MAX> = layout.axes.iter().map(|axis| axis.usage).collect();
        assert_eq!(
            usages.as_slice(),
            &[
                AxisUsage::X,
                AxisUsage::Y,
                AxisUsage::Z,
                AxisUsage::Rz,
                AxisUsage::Accelerator,
                AxisUsage::Brake
            ]
        );
    }

    #[test]
    fn q37_captured_release_has_no_dpad_direction() {
        let descriptor = HidDescriptor::parse(Q37_DESCRIPTOR).unwrap();
        let layout = descriptor.layout_for(Some(4)).unwrap();
        let released = [4, 128, 128, 128, 128, 255, 0, 0, 0, 0, 2];
        let decoded = decode_report(layout, &released).unwrap();
        assert_eq!(decoded.buttons, 0);
        assert_eq!(decoded.hat, None);
        assert_eq!(
            decoded.axes,
            [Some(128), Some(128), Some(128), Some(128), Some(0), Some(0)]
        );
        assert_eq!(neutral_report(layout).hat, None);
        assert_eq!(neutral_report(layout).axes, decoded.axes);
    }

    #[test]
    fn q37_synthetic_values_exercise_every_field_independently() {
        let descriptor = HidDescriptor::parse(Q37_DESCRIPTOR).unwrap();
        let layout = descriptor.layout_for(Some(4)).unwrap();
        // Synthetic values against the real map; not a physical trigger capture.
        let decoded =
            decode_report(layout, &[4, 0, 255, 17, 240, 2, 0x40, 0x80, 64, 192, 2]).unwrap();
        assert_eq!(decoded.buttons, 0x8040);
        assert_eq!(decoded.hat, Some(2));
        assert_eq!(
            decoded.axes,
            [Some(0), Some(255), Some(17), Some(240), Some(64), Some(192)]
        );
    }

    #[test]
    fn output_and_feature_clear_local_usages_without_advancing_input() {
        for main in [0x91, 0xb1] {
            let descriptor = HidDescriptor::parse(&[
                5, 1, 9, 5, 0xa1, 1, // Gamepad collection
                0x15, 0, 0x25, 127, 0x75, 8, 0x95, 1, 9, 0x30, main,
                2, // X belongs to Output/Feature
                9, 0x31, 0x81, 2, 0xc0, // Input contains only Y
            ])
            .unwrap();
            let layout = descriptor.layout_for(None).unwrap();
            assert_eq!(layout.axes.len(), 1);
            assert_eq!(layout.axes[0].usage, AxisUsage::Y);
            assert_eq!(layout.axes[0].bit_offset, 0);
            assert_eq!(
                decode_report(layout, &[42]).unwrap().axes,
                [Some(42), None, None, None, None, None]
            );
        }
    }

    /// A minimal, spec-legal gamepad descriptor: Generic Desktop / Joystick
    /// application collection with 8 buttons (1 bit each), a hat switch
    /// (4-bit, 0-7 with 8=null, using logical range 0..=7 to keep this test
    /// simple), and two 8-bit signed axes (X, Y). Captured-device tests
    /// complement this synthetic descriptor.
    fn joystick_descriptor() -> Vec<u8, HID_DESCRIPTOR_MAX_BYTES> {
        let items: &[u8] = &[
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x04, // Usage (Joystick)
            0xA1, 0x01, // Collection (Application)
            //   Buttons: 8 buttons, 1 bit each
            0x05, 0x09, //   Usage Page (Button)
            0x19, 0x01, //   Usage Minimum (1)
            0x29, 0x08, //   Usage Maximum (8)
            0x15, 0x00, //   Logical Minimum (0)
            0x25, 0x01, //   Logical Maximum (1)
            0x75, 0x01, //   Report Size (1)
            0x95, 0x08, //   Report Count (8)
            0x81, 0x02, //   Input (Data,Var,Abs)
            //   Hat switch: 4 bits, 0..=7 (no null-state modeling here)
            0x05, 0x01, //   Usage Page (Generic Desktop)
            0x09, 0x39, //   Usage (Hat Switch)
            0x15, 0x00, //   Logical Minimum (0)
            0x25, 0x07, //   Logical Maximum (7)
            0x75, 0x04, //   Report Size (4)
            0x95, 0x01, //   Report Count (1)
            0x81, 0x02, //   Input (Data,Var,Abs)
            //   4 bits of padding to byte-align
            0x75, 0x04, //   Report Size (4)
            0x95, 0x01, //   Report Count (1)
            0x81, 0x03, //   Input (Cnst,Ary,Abs)
            //   X, Y axes: 8-bit signed, -127..=127
            0x09, 0x30, //   Usage (X)
            0x09, 0x31, //   Usage (Y)
            0x15, 0x81, //   Logical Minimum (-127)
            0x25, 0x7F, //   Logical Maximum (127)
            0x75, 0x08, //   Report Size (8)
            0x95, 0x02, //   Report Count (2)
            0x81, 0x02, //   Input (Data,Var,Abs)
            0xC0, // End Collection
        ];
        Vec::from_slice(items).unwrap()
    }

    #[test]
    fn parses_buttons_hat_and_axes_from_a_legal_descriptor() {
        let descriptor = HidDescriptor::parse(&joystick_descriptor()).unwrap();
        assert_eq!(descriptor.layouts().len(), 1);
        let layout = descriptor.layout_for(None).unwrap();

        assert_eq!(layout.report_id, None);
        assert_eq!(layout.button_bit_offset, Some(0));
        assert_eq!(layout.button_count, 8);
        assert_eq!(layout.hat_bit_offset, Some(8));
        assert_eq!(layout.hat_bit_size, 4);
        assert_eq!(layout.axes.len(), 2);
        assert_eq!(layout.byte_len(), 4); // 1 byte buttons + 1 byte hat/pad + 2 bytes axes
    }

    #[test]
    fn decodes_a_well_formed_report() {
        let descriptor = HidDescriptor::parse(&joystick_descriptor()).unwrap();
        let layout = descriptor.layout_for(None).unwrap();

        // buttons=0b0000_0101 (buttons 1 and 3), hat=3, X=-10, Y=50.
        let report = [0b0000_0101u8, 0b0000_0011, (-10i8) as u8, 50u8];
        let decoded = decode_report(layout, &report).unwrap();

        assert_eq!(decoded.buttons, 0b0000_0101);
        assert_eq!(decoded.hat, Some(3));
        assert_eq!(decoded.axes[0], Some(-10)); // X
        assert_eq!(decoded.axes[1], Some(50)); // Y
        assert_eq!(decoded.axes[2], None);
    }

    #[test]
    fn rejects_a_truncated_report() {
        let descriptor = HidDescriptor::parse(&joystick_descriptor()).unwrap();
        let layout = descriptor.layout_for(None).unwrap();
        let short_report = [0u8; 2];
        assert_eq!(
            decode_report(layout, &short_report).unwrap_err(),
            DecodeError::Truncated
        );
    }

    #[test]
    fn rejects_an_oversize_report() {
        let descriptor = HidDescriptor::parse(&joystick_descriptor()).unwrap();
        let layout = descriptor.layout_for(None).unwrap();
        let oversize_report = [0u8; HID_REPORT_MAX_BYTES + 1];
        assert_eq!(
            decode_report(layout, &oversize_report).unwrap_err(),
            DecodeError::TooLarge
        );
    }

    #[test]
    fn neutral_report_centers_axes_and_clears_buttons() {
        let descriptor = HidDescriptor::parse(&joystick_descriptor()).unwrap();
        let layout = descriptor.layout_for(None).unwrap();

        let neutral = neutral_report(layout);
        assert_eq!(neutral.buttons, 0);
        assert_eq!(neutral.hat, None);
        assert_eq!(neutral.axes[0], Some(0)); // (-127 + 127) / 2
        assert_eq!(neutral.axes[1], Some(0));
    }

    #[test]
    fn a_descriptor_with_a_report_id_is_matched_and_mismatches_rejected() {
        let items: &[u8] = &[
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x05, // Usage (Game Pad)
            0xA1, 0x01, // Collection (Application)
            0x85, 0x07, //   Report ID (7)
            0x05, 0x09, //   Usage Page (Button)
            0x19, 0x01, //   Usage Minimum (1)
            0x29, 0x04, //   Usage Maximum (4)
            0x15, 0x00, //   Logical Minimum (0)
            0x25, 0x01, //   Logical Maximum (1)
            0x75, 0x01, //   Report Size (1)
            0x95, 0x04, //   Report Count (4)
            0x81, 0x02, //   Input (Data,Var,Abs)
            0x75, 0x04, //   Report Size (4)
            0x95, 0x01, //   Report Count (1)
            0x81, 0x03, //   Input (Cnst,Ary,Abs) -- pad to a full byte
            0xC0, // End Collection
        ];
        let descriptor = HidDescriptor::parse(items).unwrap();
        let layout = descriptor.layout_for(Some(7)).unwrap();
        assert_eq!(layout.byte_len(), 2); // report ID + 1 data byte

        let report = [7u8, 0b0000_0101];
        let decoded = decode_report(layout, &report).unwrap();
        assert_eq!(decoded.buttons, 0b0000_0101);

        let wrong_id_report = [9u8, 0b0000_0101];
        assert_eq!(
            decode_report(layout, &wrong_id_report).unwrap_err(),
            DecodeError::ReportIdMismatch
        );
    }

    #[test]
    fn accepts_a_composite_device_with_five_report_layouts() {
        let items: &[u8] = &[
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x06, 0xA1, 0x01, 0x85, 0x01, 0xC0, // keyboard, report 1
            0x09, 0x02, 0xA1, 0x01, 0x85, 0x03, 0xC0, // mouse, report 3
            0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x02, 0xC0, // consumer, report 2
            0x05, 0x0D, 0x09, 0x04, 0xA1, 0x01, 0x85, 0x05, 0xC0, // touch, report 5
            0x09, 0x02, 0xA1, 0x01, 0x85, 0x07, 0xC0, // pen, report 7
        ];

        let descriptor = HidDescriptor::parse(items).unwrap();
        let ids: Vec<Option<u8>, HID_REPORTS_MAX> = descriptor
            .layouts()
            .iter()
            .map(|layout| layout.report_id)
            .collect();
        assert_eq!(
            ids.as_slice(),
            &[Some(1), Some(3), Some(2), Some(5), Some(7)]
        );
    }

    #[test]
    fn rejects_a_descriptor_that_is_too_large() {
        let oversize = [0u8; HID_DESCRIPTOR_MAX_BYTES + 1];
        assert_eq!(
            HidDescriptor::parse(&oversize).unwrap_err(),
            DescriptorError::TooLarge
        );
    }

    #[test]
    fn rejects_a_truncated_item() {
        // A Usage Page item claiming 2 data bytes but supplying none.
        let malformed: &[u8] = &[0x06];
        assert_eq!(
            HidDescriptor::parse(malformed).unwrap_err(),
            DescriptorError::Truncated
        );
    }

    #[test]
    fn rejects_push_and_pop() {
        let items: &[u8] = &[
            0x05, 0x01, 0x09, 0x04, 0xA1, 0x01, // Application collection
            0xA4, // Push
            0xC0,
        ];
        assert_eq!(
            HidDescriptor::parse(items).unwrap_err(),
            DescriptorError::UnsupportedPushPop
        );
    }

    #[test]
    fn rejects_a_descriptor_with_no_application_collection() {
        let items: &[u8] = &[0x05, 0x01, 0x09, 0x04];
        assert_eq!(
            HidDescriptor::parse(items).unwrap_err(),
            DescriptorError::NoApplicationCollection
        );
    }
}
