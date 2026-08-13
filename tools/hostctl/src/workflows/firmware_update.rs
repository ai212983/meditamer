use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use ed25519_dalek::{Signer, SigningKey};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::{AckStatus, SerialConsole},
    workflows::common::repo_path,
};

const LEGACY_CHUNK_BYTES: usize = 48;
const STREAM_CHUNK_BYTES: usize = 112;
const STREAM_BAUD: u32 = 460_800;
const STREAM_CAPABILITY: &str = "stream=bin1@460800";
const STREAM_TRANSPORT: &str = "bin1@460800";
const STREAM_MAGIC: [u8; 2] = *b"MF";
const STREAM_VERSION: u8 = 1;
const STREAM_KIND_DATA: u8 = 1;
#[cfg(test)]
const DEVICE_UART_RX_FIFO_BYTES: usize = 128;
const OTA_SLOT_BYTES: usize = 0x1f0000;
const OTA_MIN_HEADROOM_BYTES: usize = 0x20000;
const ACK_TIMEOUT: Duration = Duration::from_secs(10);
const CHUNK_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const CHUNK_ACK_ATTEMPTS: usize = 3;
const STREAM_RETRY_BACKOFF: Duration = Duration::from_millis(50);
const BOOT_TIMEOUT: Duration = Duration::from_secs(120);
const SIGNING_DOMAIN: &[u8] = b"MEDITAMER-FIRMWARE-V1";

#[derive(Clone, Debug)]
pub struct FirmwareUpdateOptions {
    pub image: PathBuf,
    pub key: PathBuf,
    pub port: Option<String>,
    pub output: Option<PathBuf>,
    pub activate: bool,
}

struct FirmwareUpdateRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    image: Vec<u8>,
    digest: [u8; 32],
    signature: [u8; 64],
    key_id: [u8; 4],
    target: Option<String>,
    candidate_build_id: Option<String>,
    max_erase_us: Option<u32>,
    max_write_us: Option<u32>,
    verify_read_us: Option<u32>,
    activate: bool,
    transport: &'static str,
    started: Instant,
    activation_mark: usize,
    summary_path: PathBuf,
}

impl WorkflowRuntime for FirmwareUpdateRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "preflight" => self.preflight(),
            "query_status" => self.query_status(context),
            "prepare_update" => self.prepare_update(),
            "begin_update" => self.begin_update(),
            "stream_image_binary" => self.stream_image_binary_action(),
            "stream_image_legacy" => self.stream_image_legacy_action(),
            "finish_update" => self.finish_update(),
            "query_staged_status" => self.query_staged_status(),
            "activate_update" => self.activate_update(),
            "await_candidate" => self.await_candidate(),
            "write_summary" => self.write_summary(),
            other => Err(anyhow!("unsupported firmware-update action: {other}")),
        }
    }
}

impl FirmwareUpdateRuntime<'_> {
    fn preflight(&mut self) -> Result<()> {
        if self.image.len() < 256 || !self.image.len().is_multiple_of(4) || self.image[0] != 0xe9 {
            bail!("application image is not an aligned ESP application binary");
        }
        if self.image.len() > OTA_SLOT_BYTES - OTA_MIN_HEADROOM_BYTES {
            bail!(
                "application image leaves less than the accepted {}-byte OTA slot headroom",
                OTA_MIN_HEADROOM_BYTES
            );
        }
        self.console.settle(200)?;
        let boot_mark = self.console.mark();
        let pong = Regex::new(r"^PONG$")?;
        let deadline = Instant::now() + BOOT_TIMEOUT;
        let mut ready = false;
        while Instant::now() < deadline {
            if self
                .console
                .command_wait_regex("PING", &pong, Duration::from_secs(1))?
                .is_some()
            {
                ready = true;
                break;
            }
        }
        if !ready {
            bail!("serial update preflight did not reach PONG");
        }
        let runtime_ready = Regex::new(r"RUNTIME_READY app_state=ready display=ready")?;
        self.console
            .wait_for_regex_since(boot_mark, &runtime_ready, BOOT_TIMEOUT)?
            .ok_or_else(|| anyhow!("serial update preflight did not reach RUNTIME_READY"))?;
        self.logger.info(format!(
            "firmware update preflight: bytes={} sha256={} key_id={}",
            self.image.len(),
            hex(&self.digest),
            hex(&self.key_id),
        ));
        Ok(())
    }

    fn query_status(&mut self, context: &mut Value) -> Result<()> {
        let regex = Regex::new(r"FWSTATUS .* key=(configured|missing) key_id=([0-9a-f]{8})")?;
        let line = self
            .console
            .command_wait_regex("FWSTATUS", &regex, ACK_TIMEOUT)?
            .ok_or_else(|| anyhow!("FWSTATUS timed out"))?;
        if !line.contains("key=configured") {
            bail!("device has no firmware-signing public key configured");
        }
        let expected = format!("key_id={}", hex(&self.key_id));
        if !line.contains(&expected) {
            bail!("device signing key mismatch: expected {expected}; got {line}");
        }
        context["stream_available"] = Value::Bool(line.contains(STREAM_CAPABILITY));
        self.logger.info(line);
        Ok(())
    }

    fn prepare_update(&mut self) -> Result<()> {
        let (status, line) =
            self.console
                .command_wait_ack("FWPREPARE", "FWPREPARE", ACK_TIMEOUT)?;
        if status != AckStatus::Ok {
            bail!("FWPREPARE failed: {}", line.unwrap_or_default());
        }
        self.logger.info(line.unwrap_or_default());
        Ok(())
    }

    fn begin_update(&mut self) -> Result<()> {
        let command = format!(
            "FWBEGIN {} {} {}",
            self.image.len(),
            hex(&self.digest),
            hex(&self.signature),
        );
        let (status, line) = self
            .console
            .command_wait_ack(&command, "FWBEGIN", ACK_TIMEOUT)?;
        if status != AckStatus::Ok {
            bail!("FWBEGIN failed: {}", line.unwrap_or_default());
        }
        let line = line.unwrap_or_default();
        let captures = Regex::new(r"target=(ota_[01])")?
            .captures(&line)
            .ok_or_else(|| anyhow!("FWBEGIN response has no target: {line}"))?;
        self.target = Some(captures[1].to_string());
        self.logger.info(line);
        Ok(())
    }

    fn stream_image_binary_action(&mut self) -> Result<()> {
        self.transport = STREAM_TRANSPORT;
        if let Err(error) = self.stream_image_binary() {
            let baud_result = self.console.set_baud_rate(115_200);
            let reset_result = self.console.pulse_en_reset(120, 20);
            return Err(error).context(format!(
                "binary stream recovery reset issued (baud_restore={} reset={})",
                if baud_result.is_ok() { "ok" } else { "failed" },
                if reset_result.is_ok() { "ok" } else { "failed" },
            ));
        }
        Ok(())
    }

    fn stream_image_legacy_action(&mut self) -> Result<()> {
        self.transport = "hex-v1@115200";
        self.logger.info(
            "firmware stream: device has no binary capability; using compatible hex transport",
        );
        self.stream_image_legacy()
    }

    fn stream_image_legacy(&mut self) -> Result<()> {
        let total_chunks = self.image.len().div_ceil(LEGACY_CHUNK_BYTES);
        for (index, chunk) in self.image.chunks(LEGACY_CHUNK_BYTES).enumerate() {
            let offset = index * LEGACY_CHUNK_BYTES;
            let command = format!("FWCHUNK {offset} {}", hex(chunk));
            let expected_written = offset + chunk.len();
            let mut acknowledged = false;
            for attempt in 1..=CHUNK_ACK_ATTEMPTS {
                let (status, line) =
                    self.console
                        .command_wait_ack(&command, "FWCHUNK", CHUNK_ACK_TIMEOUT)?;
                if status == AckStatus::Ok {
                    let line = line.unwrap_or_default();
                    if !line.contains(&format!("written={expected_written}")) {
                        bail!("FWCHUNK returned the wrong accepted offset: {line}");
                    }
                    acknowledged = true;
                    break;
                }
                if status != AckStatus::None {
                    bail!(
                        "FWCHUNK failed explicitly at offset {offset} and was not retried: {}",
                        line.unwrap_or_default()
                    );
                }
                if attempt < CHUNK_ACK_ATTEMPTS {
                    self.logger.info(format!(
                        "firmware stream: retrying idempotent chunk offset={offset} attempt={}",
                        attempt + 1,
                    ));
                }
            }
            if !acknowledged {
                bail!(
                    "FWCHUNK timed out at offset {offset} after {CHUNK_ACK_ATTEMPTS} identical idempotent attempts"
                );
            }
            if index % 256 == 0 || index + 1 == total_chunks {
                self.logger.info(format!(
                    "firmware stream: chunk={}/{} bytes={}/{}",
                    index + 1,
                    total_chunks,
                    offset + chunk.len(),
                    self.image.len(),
                ));
            }
        }
        Ok(())
    }

    fn stream_image_binary(&mut self) -> Result<()> {
        let command = format!("FWSTREAM {STREAM_BAUD}");
        let (status, line) = self
            .console
            .command_wait_ack(&command, "FWSTREAM", BOOT_TIMEOUT)?;
        if status != AckStatus::Ok {
            bail!("FWSTREAM negotiation failed: {}", line.unwrap_or_default());
        }
        let line = line.unwrap_or_default();
        // FWSTATUS already negotiated the exact protocol and baud. Unrelated low-level runtime
        // diagnostics can interleave into this textual acknowledgement, so only its command/status
        // token is authoritative at the transition boundary.
        self.logger.info(line);
        self.console.set_baud_rate(STREAM_BAUD)?;
        std::thread::sleep(Duration::from_millis(50));

        let total_chunks = self.image.len().div_ceil(STREAM_CHUNK_BYTES);
        for (index, chunk) in self.image.chunks(STREAM_CHUNK_BYTES).enumerate() {
            let offset = index * STREAM_CHUNK_BYTES;
            let frame = build_stream_frame(offset as u32, chunk);
            let expected_written = offset + chunk.len();
            let mut acknowledged = false;
            for attempt in 1..=CHUNK_ACK_ATTEMPTS {
                let mark = self.console.mark();
                self.console.send_bytes(&frame)?;
                let (status, line) =
                    self.console
                        .wait_ack_since(mark, "FWFRAME", CHUNK_ACK_TIMEOUT)?;
                if status == AckStatus::Ok {
                    let line = line.unwrap_or_default();
                    if !line.contains(&format!("written={expected_written}")) {
                        bail!("FWFRAME returned the wrong accepted offset: {line}");
                    }
                    acknowledged = true;
                    break;
                }
                let retryable_crc = line
                    .as_deref()
                    .is_some_and(|line| line.contains("reason=crc"));
                if status != AckStatus::None && !retryable_crc {
                    bail!(
                        "FWFRAME failed explicitly at offset {offset} and was not retried: {}",
                        line.unwrap_or_default()
                    );
                }
                if attempt < CHUNK_ACK_ATTEMPTS {
                    self.logger.info(format!(
                        "binary firmware stream: retrying idempotent frame offset={offset} attempt={}",
                        attempt + 1,
                    ));
                    std::thread::sleep(STREAM_RETRY_BACKOFF);
                }
            }
            if !acknowledged {
                bail!(
                    "FWFRAME timed out at offset {offset} after {CHUNK_ACK_ATTEMPTS} identical idempotent attempts"
                );
            }
            if index % 128 == 0 || index + 1 == total_chunks {
                self.logger.info(format!(
                    "binary firmware stream: frame={}/{} bytes={}/{}",
                    index + 1,
                    total_chunks,
                    offset + chunk.len(),
                    self.image.len(),
                ));
            }
        }

        self.console.set_baud_rate(115_200)?;
        std::thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    fn finish_update(&mut self) -> Result<()> {
        let (status, line) = self
            .console
            .command_wait_ack("FWFINISH", "FWFINISH", BOOT_TIMEOUT)?;
        if status != AckStatus::Ok {
            bail!("FWFINISH failed: {}", line.unwrap_or_default());
        }
        let line = line.unwrap_or_default();
        if !line.contains(&format!("sha256={}", hex(&self.digest))) {
            bail!("FWFINISH returned the wrong digest: {line}");
        }
        self.logger.info(line);
        Ok(())
    }

    fn query_staged_status(&mut self) -> Result<()> {
        let regex = Regex::new(
            r"FWSTATUS .* phase=verified .* erase_max_us=([0-9]+) write_max_us=([0-9]+) verify_read_us=([0-9]+) multicore=transaction_park",
        )?;
        let line = self
            .console
            .command_wait_regex("FWSTATUS", &regex, ACK_TIMEOUT)?
            .ok_or_else(|| anyhow!("verified FWSTATUS timed out"))?;
        let captures = regex
            .captures(&line)
            .ok_or_else(|| anyhow!("verified FWSTATUS has no timing evidence: {line}"))?;
        let target = self.target.as_deref().unwrap_or("none");
        if !line.contains(&format!("target={target}")) {
            bail!("verified FWSTATUS reports the wrong target: {line}");
        }
        self.max_erase_us = Some(captures[1].parse()?);
        self.max_write_us = Some(captures[2].parse()?);
        self.verify_read_us = Some(captures[3].parse()?);
        self.logger.info(line);
        Ok(())
    }

    fn activate_update(&mut self) -> Result<()> {
        self.activation_mark = self.console.mark();
        let (status, line) =
            self.console
                .command_wait_ack("FWACTIVATE", "FWACTIVATE", ACK_TIMEOUT)?;
        if status != AckStatus::Ok {
            bail!(
                "FWACTIVATE did not acknowledge; outcome is ambiguous and was not retried: {}",
                line.unwrap_or_default()
            );
        }
        self.logger.info(line.unwrap_or_default());
        Ok(())
    }

    fn await_candidate(&mut self) -> Result<()> {
        let target = self
            .target
            .clone()
            .ok_or_else(|| anyhow!("missing update target"))?;
        let pending = Regex::new(&format!(
            r"FIRMWARE_BOOT booted={} selected={} state=pending_verify",
            regex::escape(&target),
            regex::escape(&target),
        ))?;
        let boot_line = self
            .console
            .wait_for_regex_since(self.activation_mark, &pending, BOOT_TIMEOUT)?
            .ok_or_else(|| anyhow!("candidate did not boot pending-verify in time"))?;
        self.candidate_build_id = Regex::new(r"build_id=([A-Za-z0-9._-]{1,31})")?
            .captures(&boot_line)
            .map(|captures| captures[1].to_owned());
        self.logger.info(boot_line);
        let ready = Regex::new(r"RUNTIME_READY app_state=ready display=ready")?;
        self.console
            .wait_for_regex_since(self.activation_mark, &ready, BOOT_TIMEOUT)?
            .ok_or_else(|| anyhow!("candidate did not reach the software health boundary"))?;
        let confirmed = Regex::new(&format!(
            r"FIRMWARE_CONFIRM slot={} state=valid",
            regex::escape(&target)
        ))?;
        self.console
            .wait_for_regex_since(self.activation_mark, &confirmed, BOOT_TIMEOUT)?
            .ok_or_else(|| anyhow!("candidate was not confirmed valid"))?;
        self.logger
            .info(format!("candidate accepted: slot={target}"));
        Ok(())
    }

    fn write_summary(&mut self) -> Result<()> {
        let summary = format!(
            "result=pass\nbytes={}\nsha256={}\nkey_id={}\ntarget={}\ncandidate_build_id={}\nerase_max_us={}\nwrite_max_us={}\nverify_read_us={}\nmulticore=transaction_park\ntransport={}\nactivated={}\nelapsed_ms={}\n",
            self.image.len(),
            hex(&self.digest),
            hex(&self.key_id),
            self.target.as_deref().unwrap_or("none"),
            self.candidate_build_id.as_deref().unwrap_or("none"),
            self.max_erase_us.unwrap_or(0),
            self.max_write_us.unwrap_or(0),
            self.verify_read_us.unwrap_or(0),
            self.transport,
            self.activate,
            self.started.elapsed().as_millis(),
        );
        fs::write(&self.summary_path, summary)?;
        self.logger.info(format!(
            "firmware update complete: {}",
            self.summary_path.display()
        ));
        Ok(())
    }
}

pub fn run_firmware_update(logger: &mut Logger, opts: FirmwareUpdateOptions) -> Result<()> {
    let image_path = repo_path(&opts.image);
    let key_path = repo_path(&opts.key);
    let image = fs::read(&image_path)
        .with_context(|| format!("read firmware image {}", image_path.display()))?;
    let signing_key = read_signing_key(&key_path)?;
    let digest: [u8; 32] = Sha256::digest(&image).into();
    let mut message = Vec::with_capacity(SIGNING_DOMAIN.len() + 4 + digest.len());
    message.extend_from_slice(SIGNING_DOMAIN);
    message.extend_from_slice(&(image.len() as u32).to_le_bytes());
    message.extend_from_slice(&digest);
    let signature = signing_key.sign(&message).to_bytes();
    let public = signing_key.verifying_key().to_bytes();
    let public_digest: [u8; 32] = Sha256::digest(public).into();
    let key_id = public_digest[..4].try_into().unwrap();

    let port = opts.port.map_or_else(env_utils::require_port, Ok)?;
    let baud = env_utils::baud_from_env(115_200)?;
    let default_log_path = format!(
        "logs/firmware_update_{}.log",
        Local::now().format("%Y%m%d_%H%M%S")
    );
    let log_path = resolve_firmware_update_log_path(
        opts.output,
        std::env::var_os("HOSTCTL_FIRMWARE_UPDATE_LOG_PATH"),
        &default_log_path,
    )?;
    ensure_parent_dir(&log_path)?;
    let summary_path = log_path.with_extension("summary.txt");
    let mut console = SerialConsole::open(&port, baud, Some(&log_path))?;
    // `poll_once` deliberately drains until one read times out. Keep the general console default
    // conservative, but do not add 50 ms to every one of tens of thousands of chunk ACKs.
    console.set_read_timeout(Duration::from_millis(2))?;
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/firmware-update.sw.yaml"),
    )?;
    let mut runtime = FirmwareUpdateRuntime {
        logger,
        console,
        image,
        digest,
        signature,
        key_id,
        target: None,
        candidate_build_id: None,
        max_erase_us: None,
        max_write_us: None,
        verify_read_us: None,
        activate: opts.activate,
        transport: "unknown",
        started: Instant::now(),
        activation_mark: 0,
        summary_path,
    };
    execute_workflow(
        &workflow,
        &mut runtime,
        &json!({ "activate": opts.activate }),
    )?;
    Ok(())
}

fn resolve_firmware_update_log_path(
    explicit: Option<PathBuf>,
    configured: Option<std::ffi::OsString>,
    default: &str,
) -> Result<PathBuf> {
    let configured_path = match explicit {
        Some(path) => path,
        None => match configured {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            Some(_) => bail!("HOSTCTL_FIRMWARE_UPDATE_LOG_PATH must not be empty"),
            None => PathBuf::from(default),
        },
    };
    Ok(repo_path(configured_path))
}

pub fn firmware_public_key_hex(path: &Path) -> Result<String> {
    Ok(hex(&read_signing_key(&repo_path(path))?
        .verifying_key()
        .to_bytes()))
}

fn read_signing_key(path: &Path) -> Result<SigningKey> {
    let raw = fs::read(path).with_context(|| format!("read signing key {}", path.display()))?;
    let bytes = if raw.len() == 32 {
        raw
    } else {
        let text = std::str::from_utf8(&raw)?.trim();
        if text.len() != 64 {
            bail!("signing key must be 32 raw bytes or 64 hex characters");
        }
        decode_hex(text.as_bytes())?
    };
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("invalid key length"))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn decode_hex(value: &[u8]) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        bail!("hex value has odd length");
    }
    value
        .chunks_exact(2)
        .map(|pair| Ok((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?))
        .collect()
}

fn hex_nibble(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => bail!("non-hex character in key"),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}

fn build_stream_frame(offset: u32, payload: &[u8]) -> Vec<u8> {
    debug_assert!(!payload.is_empty());
    debug_assert!(payload.len() <= STREAM_CHUNK_BYTES);
    debug_assert!(payload.len().is_multiple_of(4));
    let mut frame = Vec::with_capacity(10 + payload.len() + 4);
    frame.extend_from_slice(&STREAM_MAGIC);
    frame.push(STREAM_VERSION);
    frame.push(STREAM_KIND_DATA);
    frame.extend_from_slice(&offset.to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    frame.extend_from_slice(payload);
    let crc = crc32(&frame[2..]);
    frame.extend_from_slice(&crc.to_le_bytes());
    frame
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_and_hex_seed_produce_same_public_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let raw_path = dir.path().join("raw");
        let hex_path = dir.path().join("hex");
        let seed = [7u8; 32];
        fs::write(&raw_path, seed).expect("raw");
        fs::write(&hex_path, hex(&seed)).expect("hex");
        assert_eq!(
            firmware_public_key_hex(&raw_path).expect("raw public"),
            firmware_public_key_hex(&hex_path).expect("hex public")
        );
    }

    #[test]
    fn maximum_chunk_command_fits_device_uart_fifo() {
        let command = format!(
            "FWCHUNK {} {}\r\n",
            OTA_SLOT_BYTES - LEGACY_CHUNK_BYTES,
            hex(&[0; LEGACY_CHUNK_BYTES]),
        );
        assert!(command.len() <= DEVICE_UART_RX_FIFO_BYTES);
    }

    #[test]
    fn binary_frame_layout_and_crc_are_stable() {
        let frame = build_stream_frame(0x0102_0304, &[1, 2, 3, 4]);
        assert_eq!(&frame[..2], b"MF");
        assert_eq!(frame[2], 1);
        assert_eq!(frame[3], 1);
        assert_eq!(&frame[4..8], &0x0102_0304u32.to_le_bytes());
        assert_eq!(&frame[8..10], &4u16.to_le_bytes());
        assert_eq!(&frame[10..14], &[1, 2, 3, 4]);
        assert_eq!(
            u32::from_le_bytes(frame[14..18].try_into().unwrap()),
            crc32(&frame[2..14]),
        );
    }

    #[test]
    fn maximum_binary_frame_fits_device_uart_fifo() {
        let frame = build_stream_frame(0, &[0; STREAM_CHUNK_BYTES]);
        assert!(frame.len() <= DEVICE_UART_RX_FIFO_BYTES);
    }

    #[test]
    fn binary_transport_label_matches_the_negotiated_baud() {
        assert_eq!(STREAM_CAPABILITY, "stream=bin1@460800");
        assert_eq!(STREAM_TRANSPORT, "bin1@460800");
    }

    #[test]
    fn firmware_update_workflow_yaml_parses() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/firmware-update.sw.yaml");
        load_workflow(&path).expect("firmware update workflow");
    }

    #[test]
    fn firmware_update_log_env_is_honored_and_repo_relative() {
        let path = resolve_firmware_update_log_path(
            None,
            Some("logs/from-env.log".into()),
            "logs/default.log",
        )
        .expect("env log path");
        assert_eq!(path, repo_path("logs/from-env.log"));
    }

    #[test]
    fn explicit_firmware_update_log_beats_environment() {
        let path = resolve_firmware_update_log_path(
            Some(PathBuf::from("logs/from-cli.log")),
            Some("logs/from-env.log".into()),
            "logs/default.log",
        )
        .expect("explicit log path");
        assert_eq!(path, repo_path("logs/from-cli.log"));
    }

    #[test]
    fn empty_firmware_update_log_env_fails_closed() {
        let error = resolve_firmware_update_log_path(
            None,
            Some(std::ffi::OsString::new()),
            "logs/default.log",
        )
        .expect_err("empty log path");
        assert!(error
            .to_string()
            .contains("HOSTCTL_FIRMWARE_UPDATE_LOG_PATH must not be empty"));
    }
}
