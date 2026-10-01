import argparse
import asyncio
import unittest
from unittest.mock import AsyncMock, patch

import q37_rumble as q37


class RumbleTests(unittest.IsolatedAsyncioTestCase):
    async def test_channel_packets_and_stops(self):
        for channel, suffix in q37.CHANNELS.items():
            with self.subTest(channel=channel), patch.object(q37.asyncio, "sleep", new_callable=AsyncMock):
                send = AsyncMock()
                await q37.pulse(send, channel, 250)
                self.assertEqual([call.args[0] for call in send.await_args_list],
                                 [q37.STOP, q37.STOP, q37.STOP[:3] + suffix, q37.STOP, q37.STOP])

    async def test_failed_or_cancelled_start_still_stops(self):
        for error in (OSError("write failed"), asyncio.CancelledError()):
            with self.subTest(error=type(error)), patch.object(q37.asyncio, "sleep", new_callable=AsyncMock):
                send = AsyncMock(side_effect=[None, None, error, None, None])
                with self.assertRaises(type(error)):
                    await q37.pulse(send, "both", 250)
                self.assertEqual([call.args[0] for call in send.await_args_list][-2:], [q37.STOP, q37.STOP])

    async def test_stop_retries_and_reports_total_failure(self):
        with patch.object(q37.asyncio, "sleep", new_callable=AsyncMock):
            send = AsyncMock(side_effect=[OSError("first"), None])
            await q37.stop(send)
            self.assertEqual(send.await_count, 2)
            with self.assertRaisesRegex(RuntimeError, "Could not send stop"):
                await q37.stop(AsyncMock(side_effect=OSError("disconnected")))

    async def test_cancelled_pulse_wait_still_stops(self):
        with patch.object(q37.asyncio, "sleep", new_callable=AsyncMock,
                          side_effect=[None, asyncio.CancelledError(), None]):
            send = AsyncMock()
            with self.assertRaises(asyncio.CancelledError):
                await q37.pulse(send, "left", 250)
            self.assertEqual([call.args[0] for call in send.await_args_list],
                             [q37.STOP, q37.STOP, bytes.fromhex("10 05 3e ff 00"), q37.STOP, q37.STOP])

    async def test_failed_initial_stop_prevents_pulse(self):
        with patch.object(q37.asyncio, "sleep", new_callable=AsyncMock):
            send = AsyncMock(side_effect=OSError("disconnected"))
            with self.assertRaisesRegex(RuntimeError, "Could not send stop"):
                await q37.pulse(send, "both", 250)
            self.assertEqual([call.args[0] for call in send.await_args_list], [q37.STOP, q37.STOP])

    async def test_invalid_pulse_never_writes(self):
        for channel, duration in (("other", 250), ("both", 0), ("left", 1001)):
            send = AsyncMock()
            with self.assertRaises(ValueError):
                await q37.pulse(send, channel, duration)
            send.assert_not_awaited()

    def test_identity_and_duration_guards(self):
        q37.verify_identity(b"ShanWan BM-769", bytes.fromhex("01 49 19 02 04 00 00"))
        with self.assertRaises(RuntimeError):
            q37.verify_identity(b"other", bytes.fromhex("01 49 19 02 04 00 00"))
        with self.assertRaises(RuntimeError):
            q37.verify_identity(b"ShanWan BM-769", bytes.fromhex("01 49 19 03 04 00 00"))
        for value in ("0", "1001"):
            with self.assertRaises(argparse.ArgumentTypeError):
                q37.duration_ms(value)
        self.assertEqual(q37.duration_ms("250"), 250)


if __name__ == "__main__":
    unittest.main()
