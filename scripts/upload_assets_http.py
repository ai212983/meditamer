#!/usr/bin/env python3

import argparse
import http.client
import mimetypes
import os
import posixpath
import sys
from pathlib import Path
from urllib.parse import quote


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
    return parser.parse_args()


def remote_join(root: str, rel: Path) -> str:
    root = root if root.startswith("/") else f"/{root}"
    root = root.rstrip("/")
    parts = [p for p in rel.as_posix().split("/") if p not in ("", ".")]
    path = root
    for part in parts:
        path = posixpath.join(path, part)
    return path if path else "/"


def request(
    host: str,
    port: int,
    timeout: float,
    method: str,
    target: str,
    body=None,
    headers=None,
) -> bytes:
    headers = dict(headers or {})
    conn = http.client.HTTPConnection(host=host, port=port, timeout=timeout)
    try:
        conn.request(method=method, url=target, body=body, headers=headers)
        resp = conn.getresponse()
        data = resp.read()
        if resp.status // 100 != 2:
            raise RuntimeError(
                f"{method} {target} failed: {resp.status} {resp.reason} {data.decode(errors='ignore')}"
            )
        return data
    finally:
        conn.close()


def health_check(host: str, port: int, timeout: float) -> None:
    request(host, port, timeout, "GET", "/health")


def mkdir(host: str, port: int, timeout: float, remote_path: str) -> None:
    target = f"/mkdir?path={quote(remote_path, safe='/')}"
    request(host, port, timeout, "POST", target, body=b"")


def rm_path(host: str, port: int, timeout: float, remote_path: str) -> None:
    target = f"/rm?path={quote(remote_path, safe='/')}"
    request(host, port, timeout, "DELETE", target, body=b"")


def upload_file(
    host: str,
    port: int,
    timeout: float,
    local_path: Path,
    remote_path: str,
) -> None:
    size = local_path.stat().st_size
    target = f"/upload?path={quote(remote_path, safe='/')}"
    content_type, _ = mimetypes.guess_type(str(local_path))
    headers = {
        "Content-Length": str(size),
        "Content-Type": content_type or "application/octet-stream",
    }
    with local_path.open("rb") as f:
        request(host, port, timeout, "PUT", target, body=f, headers=headers)


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
        rm_path(args.host, args.port, args.timeout, remote_rm)

    if src is None:
        print("Delete complete.")
        return 0

    if src.is_file():
        remote_file = remote_join(args.dst, Path(src.name))
        remote_dir = posixpath.dirname(remote_file) or "/"
        print(f"[mkdir] {remote_dir}")
        mkdir(args.host, args.port, args.timeout, remote_dir)
        print(f"[upload] {src} -> {remote_file}")
        upload_file(args.host, args.port, args.timeout, src, remote_file)
        print("Upload complete.")
        return 0

    for rel_dir in iter_dirs(src):
        remote_dir = remote_join(args.dst, rel_dir)
        print(f"[mkdir] {remote_dir}")
        mkdir(args.host, args.port, args.timeout, remote_dir)

    for rel_file, local_file in iter_files(src):
        remote_file = remote_join(args.dst, rel_file)
        print(f"[upload] {local_file} -> {remote_file}")
        upload_file(args.host, args.port, args.timeout, local_file, remote_file)

    print("Upload complete.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
