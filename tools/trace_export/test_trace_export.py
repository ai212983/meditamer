import unittest

from trace_export import TraceFormatError, export_capture, parse_capture, render_report


def capture(rows, *, count=None, overflow=0, unsupported=0, hook_failures=0, generation=7):
    count = len(rows) if count is None else count
    return "\n".join([
        "console: booted",
        "TRACE START OK generation=7 capacity=512",
        f"TRACE BEGIN version=1 generation={generation} count={count} overflow={overflow} unsupported={unsupported} hook_failures={hook_failures} source=0",
        *rows,
        "unrelated log",
        f"TRACE END generation={generation}",
        f"TRACE STOP OK generation={generation} count={count}",
    ])


class TraceExportTests(unittest.TestCase):
    def test_valid_generic_event_preserves_name_fields_and_provenance(self):
        parsed = parse_capture(capture([
            "TRACE EVENT seq=10 ts=123 core=1 context_kind=1 context_id=4 target=power%2Frail name=sample%20ready f.volts=u:3300 f.delta=i:-2 f.ok=b:1 f.ratio=f:1.25e-1"
        ]).splitlines())
        exported = export_capture(parsed)
        event = next(item for item in exported["traceEvents"] if item.get("ph") == "i")
        self.assertEqual((event["name"], event["cat"]), ("sample ready", "power/rail"))
        self.assertEqual(event["args"], {"seq": 10, "volts": 3300, "delta": -2, "ok": True, "ratio": 0.125})
        self.assertEqual(exported["metadata"]["source"], 0)
        thread = next(item for item in exported["traceEvents"] if item.get("name") == "thread_name")
        self.assertEqual(thread["args"]["name"], "task 4")

    def test_ui_flow_names_arrows_latencies_and_zero_flow(self):
        rows = [
            "TRACE EVENT seq=0 ts=10000 core=0 context_kind=isr context_id=2 target=interaction name=stage f.stage=u:1 f.flow_id=u:42 f.a=u:7",
            "TRACE EVENT seq=1 ts=14000 core=1 context_kind=task context_id=3 target=interaction name=stage f.stage=u:7 f.flow_id=u:42 f.source_ms=u:10",
            "TRACE EVENT seq=2 ts=15000 core=1 context_kind=task context_id=3 target=interaction name=stage f.stage=u:9 f.flow_id=u:42",
            "TRACE EVENT seq=3 ts=19000 core=1 context_kind=task context_id=3 target=interaction name=stage f.stage=u:10 f.flow_id=u:42",
            "TRACE EVENT seq=4 ts=20000 core=0 context_kind=task context_id=8 target=interaction name=stage f.stage=u:4 f.flow_id=u:0 f.a=u:3",
        ]
        parsed = parse_capture(capture(rows).splitlines())
        exported = export_capture(parsed)
        instants = [e for e in exported["traceEvents"] if e.get("ph") == "i"]
        self.assertEqual([e["name"] for e in instants], ["acquired", "UI dequeued", "panel begin", "panel end", "reset"])
        arrows = [e for e in exported["traceEvents"] if e.get("ph") in {"s", "t", "f"}]
        self.assertEqual([e["ph"] for e in arrows], ["s", "t", "t", "f"])
        self.assertTrue(all(e["id"] == 42 for e in arrows))
        report = render_report(parsed)
        self.assertIn("UI queue latency: 4.000 ms", report)
        self.assertIn("Panel duration: 4.000 ms", report)

    def test_loss_counters_and_error_are_incomplete_without_inferred_drop(self):
        text = capture([], overflow=2, unsupported=3, hook_failures=4).replace(
            "TRACE END", "TRACE ERROR reason=hook\nTRACE END")
        parsed = parse_capture(text.splitlines())
        self.assertEqual(len(parsed.incomplete_reasons), 4)
        report = render_report(parsed)
        self.assertIn("overflow: 2", report)
        self.assertIn("Missing records are not interpreted as input drops", report)

    def test_additional_loss_counters_are_never_silently_ignored(self):
        text = capture([]).replace("source=0", "capacity=2 parented=3 future_loss=4")
        parsed = parse_capture(text.splitlines())
        self.assertIn("capacity field losses: 2", parsed.incomplete_reasons)
        self.assertIn("parented events: 3", parsed.incomplete_reasons)
        self.assertIn("future loss: 4", parsed.incomplete_reasons)

    def test_multiple_capture_requires_generation(self):
        text = capture([], generation=1) + "\n" + capture([], generation=2)
        with self.assertRaisesRegex(TraceFormatError, "multiple captures"):
            parse_capture(text.splitlines())
        self.assertEqual(parse_capture(text.splitlines(), generation=2).generation, 2)

    def test_rejects_wrong_version_missing_end_count_and_sequence_loss(self):
        cases = [
            capture([], generation=1).replace("version=1", "version=2"),
            "TRACE BEGIN version=1 generation=1 count=0 overflow=0 unsupported=0 hook_failures=0",
            capture([], count=1),
            capture([
                "TRACE EVENT seq=0 ts=1 core=0 context_kind=task context_id=1 target=x name=y",
                "TRACE EVENT seq=2 ts=2 core=0 context_kind=task context_id=1 target=x name=z",
            ]),
        ]
        for text in cases:
            with self.subTest(text=text):
                with self.assertRaises(TraceFormatError):
                    parse_capture(text.splitlines())

    def test_rejects_truncated_and_duplicate_sequence(self):
        malformed = capture(["TRACE EVENT seq=0 ts=1 core=0 context_kind=task context_id=1 target=x"])
        with self.assertRaisesRegex(TraceFormatError, "truncated"):
            parse_capture(malformed.splitlines())
        duplicate = capture([
            "TRACE EVENT seq=4 ts=1 core=0 context_kind=task context_id=1 target=x name=y",
            "TRACE EVENT seq=4 ts=2 core=0 context_kind=task context_id=1 target=x name=z",
        ])
        with self.assertRaisesRegex(TraceFormatError, "gap, duplicate"):
            parse_capture(duplicate.splitlines())

    def test_flows_use_timestamp_order_and_expanded_ui_stages(self):
        rows = [
            "TRACE EVENT seq=0 ts=300 core=0 context_kind=2 context_id=1 target=interaction name=stage f.stage=u:35 f.flow_id=u:5",
            "TRACE EVENT seq=1 ts=100 core=0 context_kind=2 context_id=1 target=interaction name=stage f.stage=u:30 f.flow_id=u:5",
            "TRACE EVENT seq=2 ts=200 core=0 context_kind=2 context_id=1 target=interaction name=stage f.stage=u:31 f.flow_id=u:5",
        ]
        parsed = parse_capture(capture(rows).splitlines())
        exported = export_capture(parsed)
        arrows = [event for event in exported["traceEvents"] if event.get("ph") in {"s", "t", "f"}]
        self.assertEqual([(event["ph"], event["ts"]) for event in arrows], [("s", 100), ("t", 200), ("f", 300)])
        report = render_report(parsed)
        self.assertIn("multi dequeue -> multi contacts -> panel unknown", report)
        self.assertIn("100 us (seq 1) / 300 us (seq 0)", report)

    def test_rejects_nonfinite_or_malformed_float_and_acknowledgement(self):
        for value in ("nan", "inf", "1.2.3"):
            text = capture([
                f"TRACE EVENT seq=0 ts=1 core=0 context_kind=0 context_id=1 target=x name=y f.value=f:{value}"
            ])
            with self.subTest(value=value), self.assertRaisesRegex(TraceFormatError, "finite float"):
                parse_capture(text.splitlines())
        with self.assertRaisesRegex(TraceFormatError, "acknowledgement"):
            parse_capture(("TRACE START OK\n" + capture([])).splitlines())

    def test_accepts_acknowledgements_without_ok_token(self):
        text = capture([]).replace("TRACE START OK", "TRACE START").replace("TRACE STOP OK", "TRACE STOP")
        self.assertEqual(parse_capture(text.splitlines()).generation, 7)

    def test_action_capture_control_lines_and_full_boundary(self):
        text = "TRACE ARM OK generation=7 capacity=512\n"
        for state in ("armed", "capturing", "full", "frozen"):
            text += f"TRACE STATUS state={state} generation=7 count=0\n"
        text += capture([]).replace("overflow=0", "overflow=0 buffer_full=1")
        parsed = parse_capture(text.splitlines())
        self.assertTrue(parsed.incomplete_reasons)
        for invalid in ("TRACE STATUS state=lost generation=7 count=0", "TRACE STATUS state=armed generation=7"):
            with self.subTest(invalid=invalid), self.assertRaises(TraceFormatError):
                parse_capture((invalid + "\n" + capture([])).splitlines())

    def test_rejects_live_count_regression_despite_complete_dump(self):
        status = "TRACE STATUS state=capturing generation=7 count=146\nTRACE STATUS state=capturing generation=7 count=144\n"
        with self.assertRaisesRegex(TraceFormatError, "live record count decreased"):
            parse_capture((status + capture([])).splitlines())
        text = "TRACE SELFTEST OK generation=7\n" + capture([])
        self.assertEqual(parse_capture(text.splitlines()).generation, 7)


if __name__ == "__main__":
    unittest.main()
