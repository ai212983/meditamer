#!/usr/bin/env python3
"""Send bounded Q37 motor-test pulses over an existing macOS BLE connection."""

import argparse
import asyncio
import contextlib
import json
import sys
import time
from pathlib import Path

SERVICE = "95990001-1111-6666-8888-0123456789ab"
WRITE = "95990002-1111-6666-8888-0123456789ab"
NOTIFY = "95990003-1111-6666-8888-0123456789ab"
STOP = bytes.fromhex("10 05 3e 00 00")
CHANNELS = {"left": b"\xff\x00", "right": b"\x00\xff", "both": b"\xff\xff"}


def duration_ms(value):
    value = int(value)
    if not 1 <= value <= 1000:
        raise argparse.ArgumentTypeError("duration must be 1..1000 ms")
    return value


def verify_identity(manufacturer, pnp):
    if manufacturer != b"ShanWan BM-769" or pnp != bytes.fromhex("01 49 19 02 04 00 00"):
        raise RuntimeError("Unexpected controller identity; refusing motor commands")


async def stop(send):
    """Try twice; at least one write must succeed. This is not a device ACK."""
    succeeded = False
    error = None
    for attempt in range(2):
        try:
            await send(STOP)
            succeeded = True
        except Exception as exc:
            error = exc
        if attempt == 0:
            await asyncio.sleep(0.1)
    if not succeeded:
        raise RuntimeError("Could not send stop; turn off the controller with rear Pair/Power") from error


async def pulse(send, channel, milliseconds):
    if channel not in CHANNELS or not 1 <= milliseconds <= 1000:
        raise ValueError("Invalid channel or pulse duration")
    await stop(send)
    try:
        await send(STOP[:3] + CHANNELS[channel])
        await asyncio.sleep(milliseconds / 1000)
    finally:
        # Also stop after a failed start, logging failure, or normal Ctrl-C cancellation.
        await stop(send)


async def run(args, record):
    if sys.platform != "darwin":
        raise RuntimeError("This tool currently supports macOS only")
    # Keep pure command and cleanup tests independent of macOS/PyObjC.
    from bleak import BleakClient
    from bleak.backends.corebluetooth.CentralManagerDelegate import CentralManagerDelegate
    from bleak.backends.device import BLEDevice
    from CoreBluetooth import CBUUID

    manager = CentralManagerDelegate()
    await asyncio.wait_for(manager.wait_until_ready(), 15)
    peers = manager.central_manager.retrieveConnectedPeripheralsWithServices_(
        [CBUUID.UUIDWithString_(SERVICE), CBUUID.UUIDWithString_("1812")]
    )
    peers = [peer for peer in peers if str(peer.name()) == "GamepadSpace-Q37"]
    if len(peers) != 1:
        raise RuntimeError(f"Expected one connected GamepadSpace-Q37; found {len(peers)}")
    peer = peers[0]
    device = BLEDevice(str(peer.identifier().UUIDString()), str(peer.name()), (peer, manager))
    async with BleakClient(device, timeout=20) as client:
        manufacturer = bytes(await asyncio.wait_for(
            client.read_gatt_char("00002a29-0000-1000-8000-00805f9b34fb"), 5))
        pnp = bytes(await asyncio.wait_for(
            client.read_gatt_char("00002a50-0000-1000-8000-00805f9b34fb"), 5))
        verify_identity(manufacturer, pnp)
        char = client.services.get_characteristic(WRITE)
        if char is None or char.service_uuid != SERVICE or "write-without-response" not in char.properties:
            raise RuntimeError("Expected vendor write characteristic is unavailable")
        record("connected", name=device.name, manufacturer=manufacturer.decode(), pnp=pnp.hex())

        async def send(data):
            # Do the write before logging, so a failed log cannot prevent a stop attempt.
            await asyncio.wait_for(client.write_gatt_char(char, data, response=False), 2)
            record("write_returned", uuid=WRITE, hex=data.hex(), response=False)

        if args.channel == "stop":
            await stop(send)
        else:
            await client.start_notify(NOTIFY, lambda _, data: record("notify", hex=bytes(data).hex()))
            record("pulse_requested", channel=args.channel, duration_ms=args.duration_ms)
            await pulse(send, args.channel, args.duration_ms)
        record("complete", connected=client.is_connected, physical_result="not measured")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("channel", choices=[*CHANNELS, "stop"])
    parser.add_argument("--duration-ms", type=duration_ms, default=250)
    parser.add_argument("--output", type=Path, help="New JSONL file; defaults to stdout")
    args = parser.parse_args()
    started = time.monotonic()
    destination = args.output.open("x") if args.output else contextlib.nullcontext(sys.stdout)
    with destination as output:
        def record(kind, **fields):
            output.write(json.dumps({"kind": kind, "seconds": round(time.monotonic() - started, 6), **fields}) + "\n")
            output.flush()

        try:
            asyncio.run(run(args, record))
        except KeyboardInterrupt:
            print("Interrupted; cleanup attempted the stop commands. Check the controller is quiet.", file=sys.stderr)
            return 130
        except Exception as error:
            print(f"Error: {error}", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
