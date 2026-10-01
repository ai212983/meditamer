# Offline trace exporter

Convert a captured firmware console log into Perfetto-compatible Chrome trace
JSON and a short Markdown evidence report:

```sh
python3 tools/trace_export/trace_export.py capture.log \
  --output trace.json \
  --report trace.md
```

When a log contains more than one capture, select one explicitly:

```sh
python3 tools/trace_export/trace_export.py capture.log \
  --generation 42 \
  --output trace.json \
  --report trace.md
```

The command returns 0 for complete evidence, 2 when a valid export carries
firmware-reported incompleteness counters or `TRACE ERROR`, and 1 for malformed,
truncated, or ambiguous input. An exit status of 2 still writes both outputs.
All nonzero counters other than version, generation, and event count are treated
as evidence loss so newly added loss counters cannot be overlooked.

The Python API exposes `parse_capture`, `export_capture`, and `render_report`
from the `trace_export` package. It uses only the Python standard library.

Run its tests with:

```sh
python3 -m unittest discover -s tools/trace_export -p 'test_*.py' -v
```
