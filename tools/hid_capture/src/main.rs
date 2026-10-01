//! Host HID capture and replay through the firmware's descriptor decoder.

use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

use ble::hid::{decode_report, HidDescriptor};
use hidapi::{HidApi, MAX_REPORT_DESCRIPTOR_SIZE};
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Record {
    Device {
        schema: u8,
        source: String,
        product: String,
        vendor_id: u16,
        product_id: u16,
        usage_page: u16,
        usage: u16,
        bus: String,
        descriptor: Vec<u8>,
    },
    Input {
        at_us: u64,
        data: Vec<u8>,
    },
    End {
        reports: u64,
    },
}

fn write_record(file: &mut File, record: &Record) -> Result<()> {
    serde_json::to_writer(&mut *file, record)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

fn list(filter: &str) -> Result<()> {
    let api = HidApi::new()?;
    let filter = filter.to_lowercase();
    for info in api.device_list().filter(|info| {
        info.product_string()
            .unwrap_or("")
            .to_lowercase()
            .contains(&filter)
    }) {
        println!(
            "{:04x}:{:04x} page={:04x} usage={:04x} bus={:?} {}",
            info.vendor_id(),
            info.product_id(),
            info.usage_page(),
            info.usage(),
            info.bus_type(),
            info.product_string().unwrap_or("(unnamed)"),
        );
    }
    Ok(())
}

fn parse_collection(value: &str) -> Result<(u16, u16)> {
    let (page, usage) = value
        .split_once(':')
        .ok_or("collection must be HEX-PAGE:HEX-USAGE")?;
    Ok((
        u16::from_str_radix(page, 16)?,
        u16::from_str_radix(usage, 16)?,
    ))
}

fn capture(name: &str, seconds: u64, path: &str, collection: Option<(u16, u16)>) -> Result<()> {
    if name.is_empty() || seconds == 0 || seconds > 600 {
        return Err("use a nonempty device name and a duration of 1..600 seconds".into());
    }
    let api = HidApi::new()?;
    let matches: Vec<_> = api
        .device_list()
        .filter(|info| info.product_string() == Some(name))
        .filter(|info| {
            collection
                .is_none_or(|(page, usage)| info.usage_page() == page && info.usage() == usage)
        })
        .collect();
    let [info] = matches.as_slice() else {
        return Err(format!(
            "expected one connected HID device named {name:?} with collection {collection:?}, found {}; use list first",
            matches.len()
        )
        .into());
    };
    let device = info.open_device(&api)?;
    // Capture beyond firmware limits so an unsupported descriptor is preserved,
    // not truncated to the decoder's current capacity.
    let mut descriptor = vec![0; MAX_REPORT_DESCRIPTOR_SIZE + 1];
    let size = device.get_report_descriptor(&mut descriptor)?;
    if size == 0 || size == descriptor.len() {
        return Err("empty or possibly truncated descriptor".into());
    }
    descriptor.truncate(size);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    write_record(
        &mut file,
        &Record::Device {
            schema: 1,
            source: format!("{}-hidapi", std::env::consts::OS),
            product: name.to_owned(),
            vendor_id: info.vendor_id(),
            product_id: info.product_id(),
            usage_page: info.usage_page(),
            usage: info.usage(),
            bus: format!("{:?}", info.bus_type()),
            descriptor,
        },
    )?;
    eprintln!("Capturing {name} for {seconds}s; move controls now.");
    let start = Instant::now();
    let duration = Duration::from_secs(seconds);
    let mut buffer = [0; 4097];
    let mut reports = 0;
    while start.elapsed() < duration {
        let remaining = duration.saturating_sub(start.elapsed());
        let timeout_ms = remaining.as_millis().clamp(1, 100) as i32;
        let size = device.read_timeout(&mut buffer, timeout_ms)?;
        if size == buffer.len() {
            return Err("possibly truncated input report; increase host capture buffer".into());
        }
        if size > 0 {
            write_record(
                &mut file,
                &Record::Input {
                    at_us: start.elapsed().as_micros() as u64,
                    data: buffer[..size].to_vec(),
                },
            )?;
            reports += 1;
        }
    }
    write_record(&mut file, &Record::End { reports })?;
    eprintln!("Saved {reports} reports to {path}");
    Ok(())
}

fn replay(reader: impl BufRead, mut output: impl Write) -> Result<()> {
    let mut lines = reader.lines();
    let header: Record = serde_json::from_str(&lines.next().ok_or("empty capture")??)?;
    let Record::Device {
        schema: 1,
        descriptor,
        product,
        ..
    } = header
    else {
        return Err("expected schema-1 device header".into());
    };
    let descriptor = HidDescriptor::parse(&descriptor)
        .map_err(|error| format!("firmware descriptor parser rejected {product}: {error:?}"))?;
    if descriptor.layouts().is_empty() {
        return Err("descriptor has no completed report layouts".into());
    }
    for layout in descriptor.layouts() {
        writeln!(output, "layout {layout:?}")?;
    }
    let numbered = descriptor.layouts().iter().any(|l| l.report_id.is_some());
    let mut count = 0;
    let mut previous_ticks = 0;
    let mut ended = false;
    for (index, line) in lines.enumerate() {
        if ended {
            return Err("data after end record".into());
        }
        match serde_json::from_str::<Record>(&line?)? {
            Record::Input { at_us, data } => {
                if at_us < previous_ticks {
                    return Err("capture timestamps went backwards".into());
                }
                previous_ticks = at_us;
                let id = if numbered {
                    data.first().copied()
                } else {
                    None
                };
                let layout = descriptor
                    .layout_for(id)
                    .ok_or_else(|| format!("line {}: unknown report ID {id:?}", index + 2))?;
                let decoded = decode_report(layout, &data)
                    .map_err(|error| format!("line {}: {error:?}", index + 2))?;
                writeln!(
                    output,
                    "at_us={at_us} id={id:?} buttons={:#06x} hat={:?} axes={:?}",
                    decoded.buttons, decoded.hat, decoded.axes
                )?;
                count += 1;
            }
            Record::End { reports } if reports == count => ended = true,
            Record::End { .. } => return Err("report count does not match end record".into()),
            Record::Device { .. } => return Err("unexpected second device header".into()),
        }
    }
    writeln!(output, "Decoded {count} reports; capture_complete={ended}")?;
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "list" => list(""),
        [command, filter] if command == "list" => list(filter),
        [command, name, seconds, path] if command == "capture" => {
            capture(name, seconds.parse()?, path, None)
        }
        [command, name, seconds, path, collection] if command == "capture" => {
            capture(name, seconds.parse()?, path, Some(parse_collection(collection)?))
        }
        [command, path] if command == "replay" => {
            replay(BufReader::new(File::open(path)?), std::io::stdout().lock())
        }
        _ => Err(
            "usage: hid_capture list [name-filter] | capture EXACT-NAME SECONDS FILE [HEX-PAGE:HEX-USAGE] | replay FILE"
                .into(),
        ),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replayed_inputs(capture: &[u8]) -> Vec<String> {
        let mut output = Vec::new();
        replay(capture, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        let reports: Vec<_> = output
            .lines()
            .filter(|line| line.starts_with("at_us="))
            .map(str::to_owned)
            .collect();
        assert!(output.contains(&format!(
            "Decoded {} reports; capture_complete=true",
            reports.len()
        )));
        reports
    }

    #[test]
    fn replays_captured_q37_face_buttons_and_releases() {
        let reports = replayed_inputs(include_bytes!("../fixtures/q37-face-buttons.jsonl"));
        let masks = [
            "0x0001", "0x0000", "0x0002", "0x0000", "0x0008", "0x0000", "0x0010", "0x0000",
        ];
        assert_eq!(reports.len(), masks.len());
        for (report, mask) in reports.iter().zip(masks) {
            assert!(report.contains(&format!("buttons={mask} hat=None axes=[Some(128), Some(128), Some(128), Some(128), Some(0), Some(0)]")));
        }
    }

    #[test]
    fn replays_captured_q37_shoulders_and_triggers() {
        let reports = replayed_inputs(include_bytes!("../fixtures/q37-shoulders-triggers.jsonl"));
        // L1, R1, L2 (Brake), R2 (Accelerator), each followed by release.
        let expected = [
            (0x40, 0, 0),
            (0, 0, 0),
            (0x80, 0, 0),
            (0, 0, 0),
            (0x100, 0, 255),
            (0, 0, 0),
            (0x200, 255, 0),
            (0, 0, 0),
        ];
        assert_eq!(reports.len(), expected.len());
        for (report, (buttons, accelerator, brake)) in reports.iter().zip(expected) {
            assert!(report.contains(&format!("buttons={buttons:#06x} hat=None axes=[Some(128), Some(128), Some(128), Some(128), Some({accelerator}), Some({brake})]")));
        }
    }

    #[test]
    fn replays_captured_q37_dpad_including_diagonal_transitions() {
        let reports = replayed_inputs(include_bytes!("../fixtures/q37-dpad.jsonl"));
        // Preserve brief cardinal transitions while entering/leaving diagonals.
        let hats = [
            Some(0),
            None,
            Some(1),
            Some(0),
            None,
            Some(2),
            None,
            Some(4),
            Some(3),
            Some(4),
            None,
            Some(4),
            None,
            Some(5),
            Some(4),
            None,
            Some(6),
            None,
            Some(7),
            Some(6),
            None,
        ];
        assert_eq!(reports.len(), hats.len());
        for (report, hat) in reports.iter().zip(hats) {
            assert!(report.contains(&format!("buttons=0x0000 hat={hat:?} axes=[Some(128), Some(128), Some(128), Some(128), Some(0), Some(0)]")));
        }
    }

    #[test]
    fn replays_captured_q37_stick_endpoints_partial_travel_and_centre() {
        let reports = replayed_inputs(include_bytes!("../fixtures/q37-sticks.jsonl"));
        let sticks = [
            [76, 126, 128, 128],
            [0, 126, 128, 128],
            [255, 130, 128, 128],
            [138, 0, 128, 128],
            [115, 255, 128, 128],
            [128, 128, 78, 125],
            [128, 128, 0, 119],
            [128, 128, 255, 123],
            [128, 128, 128, 0],
            [128, 128, 131, 255],
            [128, 128, 128, 128],
        ];
        assert_eq!(reports.len(), sticks.len());
        for (report, [x, y, z, rz]) in reports.iter().zip(sticks) {
            assert!(report.contains(&format!("buttons=0x0000 hat=None axes=[Some({x}), Some({y}), Some({z}), Some({rz}), Some(0), Some(0)]")));
        }
    }

    #[test]
    fn replays_captured_q37_stick_clicks_view_menu_and_combination() {
        let reports = replayed_inputs(include_bytes!("../fixtures/q37-buttons-combination.jsonl"));
        // L3, R3, View (-), Menu (+), then A+B+L1 with staggered edges.
        let masks = [
            0, 0x2000, 0, 0x4000, 0, 0x400, 0, 0x800, 0, 2, 3, 0x43, 0x41, 1, 0,
        ];
        assert_eq!(reports.len(), masks.len());
        for (report, mask) in reports.iter().zip(masks) {
            assert!(report.contains(&format!("buttons={mask:#06x} hat=None")));
        }
    }

    #[test]
    fn parses_collection_selector() {
        assert_eq!(parse_collection("1:5").unwrap(), (1, 5));
        assert_eq!(parse_collection("0c:01").unwrap(), (12, 1));
        for invalid in ["", "1", "1:2:3", "x:5", "10000:1"] {
            assert!(parse_collection(invalid).is_err());
        }
    }

    fn fixture(numbered: bool, data: &[u8], reports: u64) -> Vec<u8> {
        let mut descriptor = vec![0x05, 1, 0x09, 5, 0xa1, 1];
        if numbered {
            descriptor.extend_from_slice(&[0x85, 7]);
        }
        descriptor.extend_from_slice(&[
            0x05, 9, 0x19, 1, 0x29, 8, 0x15, 0, 0x25, 1, 0x75, 1, 0x95, 8, 0x81, 2, 0xc0,
        ]);
        let records = [
            Record::Device {
                schema: 1,
                source: "synthetic-test".into(),
                product: "test gamepad".into(),
                vendor_id: 0,
                product_id: 0,
                usage_page: 1,
                usage: 5,
                bus: "test".into(),
                descriptor,
            },
            Record::Input {
                at_us: 123,
                data: data.to_vec(),
            },
            Record::End { reports },
        ];
        let mut bytes = Vec::new();
        for record in records {
            serde_json::to_writer(&mut bytes, &record).unwrap();
            bytes.push(b'\n');
        }
        bytes
    }

    #[test]
    fn replays_numbered_and_unnumbered_hidapi_reports() {
        for (numbered, data) in [(false, &[5][..]), (true, &[7, 5][..])] {
            let mut output = Vec::new();
            replay(fixture(numbered, data, 1).as_slice(), &mut output).unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("at_us=123"));
            assert!(output.contains("buttons=0x0005"));
            assert!(output.contains("Decoded 1 reports; capture_complete=true"));
        }
    }

    #[test]
    fn rejects_bad_framing_truncation_and_capture_counts() {
        for (data, reports, expected) in [
            (&[9, 5][..], 1, "unknown report ID"),
            (&[7][..], 1, "Truncated"),
            (&[7, 5][..], 2, "report count"),
        ] {
            let error = replay(fixture(true, data, reports).as_slice(), Vec::new()).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[test]
    fn captured_q37xsp_touch_mode_exceeds_the_gamepad_decoder_fields() {
        let capture = include_bytes!("../fixtures/q37xsp-touch.jsonl");
        let error = replay(capture.as_slice(), Vec::new()).unwrap_err();
        assert!(error.to_string().contains("TooManyFields"), "{error}");
    }
}
