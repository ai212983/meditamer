#!/usr/bin/env python3

from __future__ import annotations

import argparse
import fcntl
import http.client
import os
import re
import select
import signal
import subprocess
import sys
import termios
import time
from pathlib import Path

ACTIVE_UPLOAD_PROCESS: subprocess.Popen[str] | None = None


def env_int(name: str, default: int) -> int:
    raw = os.getenv(name)
    if raw is None:
        return default
    value = int(raw)
    if value <= 0:
        raise ValueError(f"{name} must be positive")
    return value


def sanitize_label(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9._-]+", "-", value.strip())
    cleaned = cleaned.strip("-")
    if not cleaned:
        return "regression"
    return cleaned[:48]


class RunLock:
    def __init__(self, path: Path):
        self.path = path
        self.fd: int | None = None

    def acquire(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.fd = os.open(self.path, os.O_RDWR | os.O_CREAT, 0o644)
        try:
            fcntl.flock(self.fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            os.lseek(self.fd, 0, os.SEEK_SET)
            holder = os.read(self.fd, 4096).decode("utf-8", errors="replace").strip()
            raise RuntimeError(
                f"another wifi upload regression run is active (lock: {self.path}, holder: {holder or 'unknown'})"
            )
        os.ftruncate(self.fd, 0)
        os.write(self.fd, f"pid={os.getpid()} started={int(time.time())}\n".encode("utf-8"))
        os.fsync(self.fd)

    def release(self) -> None:
        if self.fd is None:
            return
        try:
            os.ftruncate(self.fd, 0)
            fcntl.flock(self.fd, fcntl.LOCK_UN)
        finally:
            os.close(self.fd)
            self.fd = None


def terminate_active_upload_process() -> None:
    global ACTIVE_UPLOAD_PROCESS
    proc = ACTIVE_UPLOAD_PROCESS
    if proc is None:
        return
    if proc.poll() is None:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=4)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait(timeout=2)
        except Exception:
            pass
    ACTIVE_UPLOAD_PROCESS = None


def install_signal_handlers() -> None:
    def _handler(signum: int, _frame) -> None:
        terminate_active_upload_process()
        raise KeyboardInterrupt(f"signal {signum}")

    signal.signal(signal.SIGINT, _handler)
    signal.signal(signal.SIGTERM, _handler)


def normalize_mac(mac: str) -> str:
    parts = [p for p in re.split(r"[^0-9a-fA-F]+", mac.strip()) if p]
    if len(parts) != 6:
        return ""
    try:
        return ":".join(f"{int(p, 16):02x}" for p in parts)
    except ValueError:
        return ""


def detect_device_mac(port: str) -> str:
    configured = normalize_mac(os.getenv("WIFI_UPLOAD_DEVICE_MAC", ""))
    if configured:
        return configured
    try:
        out = subprocess.check_output(
            ["espflash", "board-info", "-p", port, "-c", "esp32"],
            text=True,
            stderr=subprocess.STDOUT,
            timeout=5,
        )
    except Exception:
        return ""
    m = re.search(r"MAC address:\s*([0-9A-Fa-f:]+)", out)
    if not m:
        return ""
    return normalize_mac(m.group(1))


def discover_ip_by_mac(mac: str) -> str:
    if not mac:
        return ""
    try:
        out = subprocess.check_output(["arp", "-an"], text=True, timeout=2)
    except Exception:
        return ""
    for line in out.splitlines():
        m = re.search(r"\(([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)\)\s+at\s+([0-9a-fA-F:]+)", line)
        if not m:
            continue
        ip, found = m.group(1), normalize_mac(m.group(2))
        if found == mac:
            return ip
    return ""


class SerialConsole:
    def __init__(self, port: str, baud: int, log_path: Path):
        self.port = port
        self.fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        self.log_file = log_path.open("w", encoding="utf-8", errors="replace")
        self._rx = bytearray()
        self._configure(baud)
        self.drain(0.3)

    def _configure(self, baud: int) -> None:
        attrs = termios.tcgetattr(self.fd)
        attrs[0] &= ~(
            termios.IGNBRK
            | termios.BRKINT
            | termios.PARMRK
            | termios.ISTRIP
            | termios.INLCR
            | termios.IGNCR
            | termios.ICRNL
            | termios.IXON
            | termios.IXOFF
            | termios.IXANY
        )
        attrs[1] &= ~termios.OPOST
        attrs[2] &= ~(termios.PARENB | termios.CSTOPB | termios.CSIZE)
        if hasattr(termios, "CRTSCTS"):
            attrs[2] &= ~termios.CRTSCTS
        attrs[2] |= termios.CS8 | termios.CLOCAL | termios.CREAD
        attrs[3] &= ~(termios.ECHO | termios.ECHONL | termios.ICANON | termios.ISIG | termios.IEXTEN)
        attrs[4] = termios.B115200 if baud == 115200 else termios.B115200
        attrs[5] = termios.B115200 if baud == 115200 else termios.B115200
        attrs[6][termios.VMIN] = 0
        attrs[6][termios.VTIME] = 0
        termios.tcsetattr(self.fd, termios.TCSANOW, attrs)
        termios.tcflush(self.fd, termios.TCIFLUSH)

    def close(self) -> None:
        self.log_file.close()
        os.close(self.fd)

    def drain(self, seconds: float) -> None:
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            lines = self._read_lines(0.05)
            if not lines:
                continue

    def send_line(self, command: str) -> None:
        os.write(self.fd, command.encode("utf-8") + b"\r\n")

    def _read_lines(self, timeout: float) -> list[str]:
        lines: list[str] = []
        ready, _, _ = select.select([self.fd], [], [], timeout)
        if not ready:
            return lines
        try:
            chunk = os.read(self.fd, 4096)
        except BlockingIOError:
            return lines
        if not chunk:
            return lines
        self._rx.extend(chunk.replace(b"\r", b"\n"))
        while True:
            idx = self._rx.find(b"\n")
            if idx < 0:
                break
            raw = self._rx[:idx]
            del self._rx[: idx + 1]
            if not raw:
                continue
            line = raw.decode("utf-8", errors="replace").strip()
            if not line:
                continue
            self.log_file.write(line + "\n")
            self.log_file.flush()
            lines.append(line)
        return lines

    def wait_regex(self, pattern: str, timeout_s: float) -> tuple[str, list[str]] | tuple[None, list[str]]:
        rx = re.compile(pattern)
        end = time.monotonic() + timeout_s
        seen: list[str] = []
        while time.monotonic() < end:
            for line in self._read_lines(0.1):
                seen.append(line)
                if rx.search(line):
                    return line, seen
        return None, seen

    def command_wait(
        self,
        command: str,
        pattern: str,
        timeout_s: float,
    ) -> tuple[str, list[str]] | tuple[None, list[str]]:
        self.send_line(command)
        return self.wait_regex(pattern, timeout_s)


def serial_preflight(port: str, baud: int, log_path: Path) -> SerialConsole:
    last_error = "unknown"
    for attempt in range(1, 4):
        console = SerialConsole(port, baud, log_path)
        try:
            pong, _ = console.command_wait("PING", r"^PONG$", 2.5)
            if pong is not None:
                return console
            last_error = "no PONG"
        except Exception as exc:
            last_error = str(exc)
        console.close()
        if attempt == 1:
            try:
                subprocess.run(
                    ["espflash", "reset", "-p", port, "-c", "esp32"],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    timeout=5,
                )
            except Exception:
                pass
        time.sleep(1.5)
    raise RuntimeError(f"serial preflight failed: {last_error}")


def http_health(ip: str, timeout_s: int) -> bool:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        conn = http.client.HTTPConnection(ip, 8080, timeout=3)
        try:
            conn.request("GET", "/health")
            resp = conn.getresponse()
            _ = resp.read()
            if resp.status == 200:
                return True
        except Exception:
            pass
        finally:
            conn.close()
        time.sleep(1)
    return False


def wait_mode_ack(console: SerialConsole, command: str, tag: str, attempts: int = 20) -> bool:
    for _ in range(attempts):
        line, _ = console.command_wait(command, rf"^{tag} (OK|BUSY|ERR)", 4)
        if line is None:
            continue
        if f"{tag} OK" in line:
            return True
        if f"{tag} ERR" in line:
            if "reason=timeout" in line:
                time.sleep(1)
                continue
            return False
        time.sleep(1)
    return False


def maybe_wifiset(console: SerialConsole, ssid: str, password: str) -> None:
    if not ssid:
        return
    cmd = f"WIFISET {ssid}"
    if password:
        cmd = f"{cmd} {password}"
    for _ in range(8):
        line, _ = console.command_wait(cmd, r"^WIFISET (OK|BUSY|ERR)", 6)
        if line is None:
            continue
        if "WIFISET OK" in line:
            return
        if "reason=busy" in line or "WIFISET BUSY" in line:
            time.sleep(1)
            continue
        if "WIFISET ERR" in line:
            raise RuntimeError(f"WIFISET failed: {line}")
    print("WIFISET timed out after retries; continuing")


def query_metrics_net(console: SerialConsole) -> tuple[int, int, str] | None:
    console.send_line("METRICS")
    line, _ = console.wait_regex(
        r"^METRICS NET wifi_connected=([01]) http_listening=([01]) ip=([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)$",
        4,
    )
    if not line:
        return None
    m = re.search(r"wifi_connected=([01]) http_listening=([01]) ip=([0-9.]+)", line)
    if not m:
        return None
    return int(m.group(1)), int(m.group(2)), m.group(3)


def wait_network_ready(
    console: SerialConsole, connect_timeout_s: int, listen_timeout_s: int
) -> tuple[int, int, str]:
    started = time.monotonic()
    connect_deadline = started + connect_timeout_s
    listen_deadline = started + listen_timeout_s
    connect_ms = 0
    while time.monotonic() < listen_deadline:
        metrics = query_metrics_net(console)
        if not metrics:
            time.sleep(1)
            continue
        wifi_connected, listening, ip = metrics
        now_ms = int((time.monotonic() - started) * 1000)
        if wifi_connected and connect_ms == 0:
            connect_ms = now_ms
        if listening and ip != "0.0.0.0":
            if connect_ms == 0:
                connect_ms = now_ms
            return connect_ms, now_ms, ip
        if connect_ms == 0 and time.monotonic() > connect_deadline:
            raise RuntimeError(f"wifi did not connect within {connect_timeout_s}s")
        time.sleep(1)
    raise RuntimeError(f"upload server did not start within {listen_timeout_s}s")


def verify_remote_file(console: SerialConsole, remote_path: str, timeout_ms: int) -> bool:
    for _ in range(8):
        line, _ = console.command_wait(f"SDFATSTAT {remote_path}", r"^SDFATSTAT (OK|BUSY|ERR)", 4)
        if not line:
            continue
        if "SDFATSTAT ERR" in line:
            return False
        if "SDFATSTAT BUSY" in line:
            time.sleep(1)
            continue
        req_line, _ = console.wait_regex(r"^SDREQ id=([0-9]+) op=fat_stat", 8)
        if not req_line:
            continue
        m = re.search(r"id=([0-9]+)", req_line)
        if not m:
            continue
        req_id = int(m.group(1))
        wait_s = timeout_ms / 1000.0 + 10
        done_line, _ = console.command_wait(
            f"SDWAIT {req_id} {timeout_ms}",
            r"^SDWAIT (DONE|TIMEOUT|ERR)",
            wait_s,
        )
        if done_line and "SDWAIT DONE" in done_line and "status=ok" in done_line and "code=ok" in done_line:
            return True
    return False


def run_upload(host_ip: str, payload_path: str, cycle_remote_root: str, timeout_s: int) -> None:
    global ACTIVE_UPLOAD_PROCESS
    payload_bytes = Path(payload_path).stat().st_size
    # SD append path can be slow on this platform (many 1KiB roundtrips).
    # Budget timeout by payload size to avoid false negatives in regression runs.
    per_kib_budget_s = float(os.getenv("WIFI_UPLOAD_SUBPROCESS_SEC_PER_KIB", "1.0"))
    floor_s = int(os.getenv("WIFI_UPLOAD_SUBPROCESS_TIMEOUT_FLOOR_SEC", "60"))
    ceiling_s = int(os.getenv("WIFI_UPLOAD_SUBPROCESS_TIMEOUT_CEIL_SEC", "900"))
    payload_budget_s = int((payload_bytes / 1024.0) * per_kib_budget_s)
    subprocess_timeout_s = max(timeout_s * 4, floor_s, payload_budget_s)
    subprocess_timeout_s = min(subprocess_timeout_s, ceiling_s)

    cmd = [
        sys.executable,
        "-u",
        str(Path(__file__).with_name("upload_assets_http.py")),
        "--host",
        host_ip,
        "--port",
        "8080",
        "--src",
        payload_path,
        "--dst",
        cycle_remote_root,
        "--timeout",
        str(timeout_s),
    ]
    process: subprocess.Popen[str] | None = None
    try:
        process = subprocess.Popen(
            cmd,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        ACTIVE_UPLOAD_PROCESS = process
        stdout_data, stderr_data = process.communicate(timeout=subprocess_timeout_s)
        completed = subprocess.CompletedProcess(
            cmd, process.returncode, stdout=stdout_data, stderr=stderr_data
        )
    except subprocess.TimeoutExpired as exc:
        if process is not None:
            try:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=4)
            except Exception:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except Exception:
                    pass
        if exc.stdout:
            out = exc.stdout.decode("utf-8", errors="replace") if isinstance(exc.stdout, bytes) else exc.stdout
            sys.stderr.write(out)
        if exc.stderr:
            err = exc.stderr.decode("utf-8", errors="replace") if isinstance(exc.stderr, bytes) else exc.stderr
            sys.stderr.write(err)
        raise RuntimeError(f"upload subprocess timed out ({subprocess_timeout_s}s)")
    finally:
        ACTIVE_UPLOAD_PROCESS = None
    if completed.returncode != 0:
        sys.stderr.write(completed.stdout)
        sys.stderr.write(completed.stderr)
        raise RuntimeError("upload failed")


def resolve_upload_ip(metrics_ip: str, mac: str, discovery_timeout_s: int) -> str:
    if not mac:
        return metrics_ip
    try:
        subprocess.run(["ping", "-c", "1", "-W", "1000", metrics_ip], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except Exception:
        pass
    deadline = time.monotonic() + discovery_timeout_s
    while time.monotonic() < deadline:
        ip = discover_ip_by_mac(mac)
        if ip:
            return ip
        time.sleep(1)
    return metrics_ip


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("output_path", nargs="?", default="")
    parser.add_argument(
        "--test-name",
        default=os.getenv("WIFI_UPLOAD_TEST_NAME", "regression"),
        help="label for this run, used in logs and output",
    )
    args = parser.parse_args()

    install_signal_handlers()

    port = os.getenv("ESPFLASH_PORT", "").strip()
    if not port:
        raise SystemExit("ESPFLASH_PORT must be set")
    test_name = sanitize_label(args.test_name)
    lock_path = Path(
        os.getenv(
            "WIFI_UPLOAD_LOCK_PATH",
            f"/tmp/meditamer_wifi_upload_{test_name}.lock",
        )
    )
    lock = RunLock(lock_path)

    cycles = env_int("WIFI_UPLOAD_CYCLES", 3)
    payload_bytes = env_int("WIFI_UPLOAD_PAYLOAD_BYTES", 524288)
    connect_timeout_s = env_int("WIFI_UPLOAD_CONNECT_TIMEOUT_SEC", 45)
    listen_timeout_s = env_int("WIFI_UPLOAD_LISTEN_TIMEOUT_SEC", 75)
    upload_timeout_s = env_int("WIFI_UPLOAD_HTTP_TIMEOUT_SEC", 30)
    health_timeout_s = env_int("WIFI_UPLOAD_HEALTH_TIMEOUT_SEC", 15)
    stat_timeout_ms = env_int("WIFI_UPLOAD_STAT_TIMEOUT_MS", 30000)
    ip_discovery_timeout_s = env_int("WIFI_UPLOAD_IP_DISCOVERY_TIMEOUT_SEC", 8)
    baud = env_int("ESPFLASH_BAUD", 115200)
    remote_root = os.getenv("WIFI_UPLOAD_REMOTE_ROOT", "/assets/u")
    payload_path = os.getenv("WIFI_UPLOAD_PAYLOAD_PATH", "/tmp/u.bin")
    ssid = os.getenv("WIFI_UPLOAD_SSID", "")
    password = os.getenv("WIFI_UPLOAD_PASSWORD", "")

    if args.output_path:
        log_path = Path(args.output_path)
    else:
        log_path = Path(__file__).resolve().parent.parent / "logs" / f"wifi_upload_{test_name}_{time.strftime('%Y%m%d_%H%M%S')}.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)

    mac = detect_device_mac(port)
    if mac:
        print(f"Device MAC for host IP discovery: {mac}")
    print(f"Test name: {test_name}")

    print(f"Preparing upload payload: {payload_path} ({payload_bytes} bytes)")
    Path(payload_path).parent.mkdir(parents=True, exist_ok=True)
    data = bytearray(payload_bytes)
    for i in range(payload_bytes):
        data[i] = (i * 31 + 17) & 0xFF
    Path(payload_path).write_bytes(data)
    payload_kib = payload_bytes / 1024.0
    min_upload_kib_s = float(os.getenv("WIFI_UPLOAD_MIN_KIB_PER_SEC", "0.15"))
    timeout_floor_s = int(os.getenv("WIFI_UPLOAD_TIMEOUT_FLOOR_SEC", "60"))
    timeout_ceil_s = int(os.getenv("WIFI_UPLOAD_TIMEOUT_CEIL_SEC", "3600"))
    payload_timeout_s = int(payload_kib / max(min_upload_kib_s, 0.01))
    effective_upload_timeout_s = max(upload_timeout_s, timeout_floor_s, payload_timeout_s)
    effective_upload_timeout_s = min(effective_upload_timeout_s, timeout_ceil_s)
    print(
        f"Upload timeout budget: requested={upload_timeout_s}s effective={effective_upload_timeout_s}s "
        f"payload_kib={payload_kib:.1f} min_kib_s={min_upload_kib_s:.2f}"
    )

    console: SerialConsole | None = None
    try:
        lock.acquire()
        console = serial_preflight(port, baud, log_path)

        total_start = time.monotonic()
        connect_samples: list[int] = []
        listen_samples: list[int] = []
        upload_samples: list[int] = []
        throughput_samples: list[float] = []

        for cycle in range(1, cycles + 1):
            print(f"\n=== cycle {cycle}/{cycles} ===")
            if cycle > 1:
                wait_mode_ack(console, "MODE UPLOAD OFF", "MODE", 12)
                time.sleep(1)

            if not wait_mode_ack(console, "MODE UPLOAD ON", "MODE", 20):
                raise RuntimeError("MODE UPLOAD ON did not return OK")
            maybe_wifiset(console, ssid, password)
            connect_ms, listen_ms, metrics_ip = wait_network_ready(console, connect_timeout_s, listen_timeout_s)
            ip = resolve_upload_ip(metrics_ip, mac, ip_discovery_timeout_s)
            if not http_health(ip, health_timeout_s):
                refreshed = resolve_upload_ip(metrics_ip, mac, ip_discovery_timeout_s)
                if refreshed != ip and http_health(refreshed, health_timeout_s):
                    ip = refreshed
                else:
                    raise RuntimeError(f"/health did not respond for ip={ip}")

            cycle_root = f"{remote_root}/cycle-{cycle}"
            remote_file = f"{cycle_root}/{Path(payload_path).name}"
            if len(remote_file) > 64:
                raise RuntimeError(f"remote path exceeds SD_PATH_MAX(64): {remote_file}")

            started = time.monotonic()
            run_upload(ip, payload_path, cycle_root, effective_upload_timeout_s)
            upload_ms = int((time.monotonic() - started) * 1000)
            if not verify_remote_file(console, remote_file, stat_timeout_ms):
                raise RuntimeError(f"SD verification failed for {remote_file}")

            kib_s = (payload_bytes / 1024.0) / max(0.001, upload_ms / 1000.0)
            connect_samples.append(connect_ms)
            listen_samples.append(listen_ms)
            upload_samples.append(upload_ms)
            throughput_samples.append(kib_s)
            print(
                f"[PASS] cycle {cycle} ip={ip} connect_ms={connect_ms} listen_ms={listen_ms} "
                f"upload_ms={upload_ms} throughput_kib_s={kib_s:.2f}"
            )

        total_ms = int((time.monotonic() - total_start) * 1000)
        print("\nWi-Fi/upload regression summary")
        print(f"  cycles={cycles} payload_bytes={payload_bytes} total_ms={total_ms}")
        print(f"  connect_ms avg={sum(connect_samples)/len(connect_samples):.2f} min={min(connect_samples)} max={max(connect_samples)}")
        print(f"  listen_ms  avg={sum(listen_samples)/len(listen_samples):.2f} min={min(listen_samples)} max={max(listen_samples)}")
        print(f"  upload_ms  avg={sum(upload_samples)/len(upload_samples):.2f} min={min(upload_samples)} max={max(upload_samples)}")
        print(
            f"  throughput_kib_s avg={sum(throughput_samples)/len(throughput_samples):.2f} "
            f"min={min(throughput_samples):.2f} max={max(throughput_samples):.2f}"
        )
        print(f"  test_name={test_name}")
        print(f"Log: {log_path}")
        return 0
    except Exception as exc:
        print(f"[FAIL] {exc}", file=sys.stderr)
        print(f"Log: {log_path}", file=sys.stderr)
        return 1
    finally:
        terminate_active_upload_process()
        if console is not None:
            console.close()
        lock.release()


if __name__ == "__main__":
    raise SystemExit(main())
