# Upload SD assets over Wi-Fi

Set `$DEVICE_PORT` to the target. [Network reference](../../references/network-and-upload.md)
owns command fields, credentials, HTTP routes, tokens and buffer controls.
[Network validation](validation.md) owns acceptance/regression.

## Build and provision

For Inkplate, use the ordinary production build/flash. For Medinote/S3:

```bash
MEDITAMER_UPLOAD_HTTP_TOKEN="$UPLOAD_HTTP_TOKEN" \
  targets/medinote-waveshare/build.sh --locked --bin medinote-waveshare \
  --features crash-screen,wifi-storage
```

For its flash wrapper, set `MEDINOTE_FEATURES=crash-screen,wifi-storage` and the
same token environment. Keep tokens untracked and out of logs/transcripts.

Provision over UART using `NETCFG SET` with SSID/password and any required policy
fields from the [template](../../../tools/hostctl/scenarios/wifi-policy.default.json).
Query NETCFG GET to check application. Do not capture plaintext passwords in
tracked evidence. Inkplate requires a full flash when installing `wifi_config`
for the first time; app-only updates do not add the partition.

Host hardware wrappers load untracked `.env.local`, copied from `.env.example`,
with `HOSTCTL_NET_SSID`, `HOSTCTL_NET_PASSWORD`, port and baud. Direct hostctl
calls require the environment to be supplied explicitly.

## Enable, upload, disable

On Inkplate, enable `STATE SET upload=on`; Medinote uses NET START or its Wi-Fi app.
Provision first, send NET START once, and poll NET STATUS until Ready with nonzero
IPv4 and listener availability. Missing START/STOP acknowledgement is ambiguous;
inspect the capture before repeating the command.

```bash
curl "http://<device-ip>:8080/health"
HOSTCTL_UPLOAD_TOKEN="$UPLOAD_HTTP_TOKEN" \
  scripts/hostctl.sh upload --host <device-ip> --src assets --dst /assets
```

`--src` accepts a directory or one file. Relative paths resolve from repository
root. For deletion, paths are relative to `--dst` or absolute under `/assets`:

```bash
scripts/hostctl.sh upload --host <device-ip> --dst /assets --rm old.bin --rm unused/
```

Directory mutation examples (omit the header only on an optional-token Inkplate):

```bash
curl -X POST -H "x-upload-token: ${UPLOAD_HTTP_TOKEN}" \
  "http://<device-ip>:8080/mkdir?path=/assets/images"
curl -X DELETE -H "x-upload-token: ${UPLOAD_HTTP_TOKEN}" \
  "http://<device-ip>:8080/rm?path=/assets/old.bin"
```

Disable Inkplate upload using STATE SET upload=off; stop Medinote using NET STOP.
Restoring prior service state is part of a bounded diagnostic run.

## Cancellation and recovery check

On an already ready Inkplate:

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test sd-upload-recovery \
  --firmware-elf logs/<recorded-flash>/firmware.elf --output logs/<new-recovery-run>
```

One passive connection opens a unique 64-byte upload, waits for SD Begin, then
interrupts the body. The workflow requires Abort completion and absence of both
destination/temp files before a distinct upload, HTTP 201, and 64-byte readback.
It restores SD logging, does not retry failures, and retains the successful probe
file. The ELF associates recorded flash evidence; it is not device flash readback.
This checks session cancellation, not in-flight DMA cancellation, sustained
throughput, or sensor responsiveness. Run the network regression gate before
landing the change.
