#!/usr/bin/env python3

import argparse
import http.client
import os
import posixpath
import sys
import time
from pathlib import Path
from urllib.parse import quote

UPLOAD_CHUNK_SIZE = int(os.getenv("UPLOAD_CHUNK_SIZE", "8192"))
UPLOAD_NET_RECOVERY_TIMEOUT_S = float(
    os.getenv("UPLOAD_NET_RECOVERY_TIMEOUT_SEC", "45")
)
UPLOAD_NET_RECOVERY_POLL_S = float(os.getenv("UPLOAD_NET_RECOVERY_POLL_SEC", "0.8"))
UPLOAD_SD_BUSY_TOTAL_RETRY_S = float(os.getenv("UPLOAD_SD_BUSY_TOTAL_RETRY_SEC", "180"))
UPLOAD_CONNECT_TIMEOUT_S = float(os.getenv("UPLOAD_CONNECT_TIMEOUT_SEC", "4"))
UPLOAD_SKIP_MKDIR = os.getenv("UPLOAD_SKIP_MKDIR", "0") == "1"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Upload assets to device SD card over HTTP (STA mode)."
    )
    parser.add_argument("--host", required=True, help="Device IP or hostname")
    parser.add_argument("--port", type=int, default=8080, help="HTTP port (default: 8080)")
    parser.add_argument("--src", help="Local source file or directory to upload")
    parser.add_argument(
        "--dst",
        default="/assets",
        help="Remote destination root on SD card (default: /assets)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=60.0,
        help="HTTP timeout in seconds (default: 60)",
    )
    parser.add_argument(
        "--rm",
        action="append",
        default=[],
        metavar="REMOTE_PATH",
        help="Remote path to delete (absolute or relative to --dst); can be repeated",
    )
    parser.add_argument(
        "--token",
        default=os.getenv("UPLOAD_TOKEN"),
        help="Upload auth token for x-upload-token header (default: UPLOAD_TOKEN env var)",
    )
    return parser.parse_args()


def remote_join(root: str, rel: Path) -> str:
    root = root if root.startswith("/") else f"/{root}"
    root = root.rstrip("/")
    parts = [p for p in rel.as_posix().split("/") if p not in ("", ".")]
    path = root
    for part in parts:
        path = posixpath.join(path, part)
    return path if path else "/"


def auth_headers(token: str | None, headers=None) -> dict[str, str]:
    merged = dict(headers or {})
    if token:
        merged["x-upload-token"] = token
    return merged


def request(
    host: str,
    port: int,
    timeout: float,
    method: str,
    target: str,
    body=None,
    headers=None,
    retries: int = 1,
    retry_delay: float = 0.2,
) -> bytes:
    headers = dict(headers or {})
    attempts = max(1, retries)
    last_exc = None
    for attempt in range(attempts):
        connect_timeout = min(timeout, max(0.5, UPLOAD_CONNECT_TIMEOUT_S))
        conn = http.client.HTTPConnection(host=host, port=port, timeout=connect_timeout)
        try:
            if attempt > 0 and hasattr(body, "seek"):
                body.seek(0)
            # Keep connect timeout short so transient reachability loss does not
            # pin the uploader in long SYN_SENT states.
            conn.connect()
            if conn.sock is not None:
                conn.sock.settimeout(timeout)
            conn.request(method=method, url=target, body=body, headers=headers)
            resp = conn.getresponse()
            data = resp.read()
            if resp.status // 100 != 2:
                raise RuntimeError(
                    f"{method} {target} failed: {resp.status} {resp.reason} {data.decode(errors='ignore')}"
                )
            return data
        except (
            ConnectionRefusedError,
            ConnectionResetError,
            TimeoutError,
            OSError,
            http.client.HTTPException,
        ) as exc:
            last_exc = exc
            if attempt + 1 >= attempts:
                raise
            time.sleep(retry_delay * (attempt + 1))
        finally:
            conn.close()
    if last_exc is not None:
        raise last_exc
    raise RuntimeError("request failed without an exception")

def request_sd_busy_aware(
    host: str,
    port: int,
    timeout: float,
    method: str,
    target: str,
    body=None,
    headers=None,
    retries: int = 1,
    busy_retries: int = 8,
) -> bytes:
    attempts = max(1, busy_retries)
    deadline = time.monotonic() + max(1.0, UPLOAD_SD_BUSY_TOTAL_RETRY_S)
    for attempt in range(10_000):
        retries_remaining = attempt + 1 < attempts
        time_remaining = time.monotonic() < deadline
        can_retry = retries_remaining or time_remaining
        try:
            return request(
                host,
                port,
                timeout,
                method,
                target,
                body=body,
                headers=headers,
                retries=retries,
            )
        except (
            ConnectionRefusedError,
            ConnectionResetError,
            TimeoutError,
            OSError,
            http.client.HTTPException,
        ):
            if not can_retry:
                raise
            _wait_for_network_recovery(
                host,
                port,
                timeout=min(timeout, 4.0),
                window_s=min(UPLOAD_NET_RECOVERY_TIMEOUT_S, max(0.0, deadline - time.monotonic())),
            )
            time.sleep(0.25 * (attempt + 1))
            continue
        except RuntimeError as exc:
            message = str(exc)
            if "408 Request Timeout" in message:
                if not can_retry:
                    raise
                _wait_for_network_recovery(
                    host,
                    port,
                    timeout=min(timeout, 4.0),
                    window_s=min(UPLOAD_NET_RECOVERY_TIMEOUT_S, max(0.0, deadline - time.monotonic())),
                )
                time.sleep(0.25 * (attempt + 1))
                continue
            if "409 Conflict" in message and "sd busy" in message:
                try:
                    request(
                        host,
                        port,
                        timeout,
                        "POST",
                        "/upload_abort",
                        body=b"",
                        headers=headers,
                        retries=2,
                    )
                except Exception:
                    pass
                if not can_retry:
                    raise
                time.sleep(0.25 * (attempt + 1))
                continue
            raise
    raise RuntimeError(f"{method} {target} failed: sd busy persisted")


def _wait_for_network_recovery(
    host: str,
    port: int,
    timeout: float,
    window_s: float,
) -> bool:
    if window_s <= 0:
        return False
    deadline = time.monotonic() + window_s
    while time.monotonic() < deadline:
        try:
            request(host, port, timeout, "GET", "/health", retries=1)
            return True
        except Exception:
            time.sleep(UPLOAD_NET_RECOVERY_POLL_S)
    return False


def health_check(host: str, port: int, timeout: float) -> None:
    request(host, port, timeout, "GET", "/health", retries=20)


def mkdir(host: str, port: int, timeout: float, remote_path: str, token: str | None) -> None:
    target = f"/mkdir?path={quote(remote_path, safe='/')}"
    request_sd_busy_aware(
        host,
        port,
        timeout,
        "POST",
        target,
        body=b"",
        headers=auth_headers(token),
        retries=8,
    )

def mkdir_p(host: str, port: int, timeout: float, remote_path: str, token: str | None) -> None:
    if not remote_path.startswith("/"):
        remote_path = f"/{remote_path}"
    parts = [p for p in remote_path.split("/") if p]
    if not parts:
        return
    current = ""
    for part in parts:
        current = f"{current}/{part}"
        mkdir(host, port, timeout, current, token)


def rm_path(host: str, port: int, timeout: float, remote_path: str, token: str | None) -> None:
    target = f"/rm?path={quote(remote_path, safe='/')}"
    request_sd_busy_aware(
        host,
        port,
        timeout,
        "DELETE",
        target,
        body=b"",
        headers=auth_headers(token),
        retries=8,
    )


def upload_file(
    host: str,
    port: int,
    timeout: float,
    local_path: Path,
    remote_path: str,
    token: str | None,
) -> None:
    size = local_path.stat().st_size
    upload_target = f"/upload?path={quote(remote_path, safe='/')}"
    upload_headers = auth_headers(
        token,
        {
            "Content-Length": str(size),
            "Content-Type": "application/octet-stream",
        },
    )
    with local_path.open("rb") as body:
        try:
            request_sd_busy_aware(
                host,
                port,
                timeout,
                "PUT",
                upload_target,
                body=body,
                headers=upload_headers,
                retries=4,
                busy_retries=6,
            )
            return
        except Exception as exc:
            print(
                f"[upload] single-shot PUT failed ({exc}); falling back to /upload_chunk flow",
                file=sys.stderr,
            )
            # Fall back to legacy chunked upload flow when single-shot PUT
            # is unavailable or unstable under current link conditions.
            try:
                request(
                    host,
                    port,
                    timeout,
                    "POST",
                    "/upload_abort",
                    body=b"",
                    headers=auth_headers(token),
                    retries=2,
                )
            except Exception:
                pass

    # Legacy fallback path for firmware variants without /upload support,
    # or as a resilience fallback after PUT failures.
    begin_target = f"/upload_begin?path={quote(remote_path, safe='/')}&size={size}"
    request_sd_busy_aware(
        host,
        port,
        timeout,
        "POST",
        begin_target,
        body=b"",
        headers=auth_headers(token),
        retries=8,
        busy_retries=6,
    )
    try:
        with local_path.open("rb") as f:
            while True:
                chunk = f.read(UPLOAD_CHUNK_SIZE)
                if not chunk:
                    break
                headers = auth_headers(
                    token,
                    {
                        "Content-Length": str(len(chunk)),
                        "Content-Type": "application/octet-stream",
                    },
                )
                request_sd_busy_aware(
                    host,
                    port,
                    timeout,
                    "PUT",
                    "/upload_chunk",
                    body=chunk,
                    headers=headers,
                    retries=5,
                    busy_retries=6,
                )
        request_sd_busy_aware(
            host,
            port,
            timeout,
            "POST",
            "/upload_commit",
            body=b"",
            headers=auth_headers(token),
            retries=8,
            busy_retries=6,
        )
    except Exception:
        try:
            request(
                host,
                port,
                timeout,
                "POST",
                "/upload_abort",
                body=b"",
                headers=auth_headers(token),
                retries=3,
            )
        except Exception:
            pass
        raise


def iter_files(src: Path):
    if src.is_file():
        yield Path("."), src
        return

    for root, _, files in os.walk(src):
        root_path = Path(root)
        rel_root = root_path.relative_to(src)
        for name in sorted(files):
            local_file = root_path / name
            rel_file = rel_root / name
            yield rel_file, local_file


def iter_dirs(src: Path):
    if src.is_file():
        return
    yield Path(".")
    for root, dirs, _ in os.walk(src):
        root_path = Path(root)
        rel_root = root_path.relative_to(src)
        for name in sorted(dirs):
            yield rel_root / name


def main() -> int:
    args = parse_args()
    token = args.token
    src = Path(args.src).resolve() if args.src else None
    if src is None and not args.rm:
        print("Nothing to do: provide --src and/or --rm", file=sys.stderr)
        return 2
    if src is not None and not src.exists():
        print(f"Source path does not exist: {src}", file=sys.stderr)
        return 2

    try:
        health_check(args.host, args.port, args.timeout)
    except Exception as exc:
        print(f"Health check failed: {exc}", file=sys.stderr)
        return 3

    for raw_rm in args.rm:
        remote_rm = raw_rm if raw_rm.startswith("/") else remote_join(args.dst, Path(raw_rm))
        print(f"[delete] {remote_rm}")
        rm_path(args.host, args.port, args.timeout, remote_rm, token)

    if src is None:
        print("Delete complete.")
        return 0

    if src.is_file():
        remote_file = remote_join(args.dst, Path(src.name))
        remote_dir = posixpath.dirname(remote_file) or "/"
        if UPLOAD_SKIP_MKDIR:
            print(f"[mkdir -p] skipped ({remote_dir})")
        else:
            print(f"[mkdir -p] {remote_dir}")
            mkdir_p(args.host, args.port, args.timeout, remote_dir, token)
        print(f"[upload] {src} -> {remote_file}")
        upload_file(args.host, args.port, args.timeout, src, remote_file, token)
        print("Upload complete.")
        return 0

    created_dirs = set()
    for rel_dir in iter_dirs(src):
        remote_dir = remote_join(args.dst, rel_dir)
        if remote_dir in created_dirs:
            continue
        print(f"[mkdir -p] {remote_dir}")
        mkdir_p(args.host, args.port, args.timeout, remote_dir, token)
        created_dirs.add(remote_dir)

    for rel_file, local_file in iter_files(src):
        remote_file = remote_join(args.dst, rel_file)
        print(f"[upload] {local_file} -> {remote_file}")
        upload_file(args.host, args.port, args.timeout, local_file, remote_file, token)

    print("Upload complete.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
