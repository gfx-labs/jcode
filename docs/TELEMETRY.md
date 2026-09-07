# Telemetry reporting destinations

Jcode sends usage events to `https://telemetry.jcode.sh/v1/event` by default.
Transcript uploads use `/v1/transcript` and require separate content-sharing
consent. Changing the destination does not enable either kind of reporting.

## Local or self-hosted receiver

Set one environment variable to redirect **both** streams:

```bash
export JCODE_TELEMETRY_BASE_URL=http://127.0.0.1:8080
jcode telemetry status
```

The receiver should accept JSON POST requests at `/v1/event` and, if content
sharing is enabled, `/v1/transcript`, returning a 2xx status. These are Jcode's
custom event schemas, **not OpenTelemetry/OTLP**. See
`crates/jcode-usage-types/src/lib.rs` for usage event fields and
`crates/jcode-telemetry-core/src/lib.rs` for transcript payload construction.

A path prefix is supported: `https://collector.example/jcode` produces
`/jcode/v1/event` and `/jcode/v1/transcript`. Supply the base, not the full
`/v1/event` path. Trailing slashes are ignored. HTTP and HTTPS are supported.
Use HTTP only on a trusted local network, preferably loopback. Use HTTPS for
remote receivers. Credentials in URLs, query strings, and fragments are rejected.

Set the variable in the **daemon/server process environment**, not just in a
client attaching to an already-running daemon. Restart the daemon with that
environment for server-side events to move. SSH sessions need it on the SSH
host, where the daemon runs. To persist the override, put it in the shell or
service configuration that launches Jcode. There is no TOML setting for this
override.

`jcode telemetry status --json` includes `event_endpoint`,
`transcript_endpoint`, and `endpoint_error`. It is read-only and creates no
telemetry identity. Consent status (`enabled`) is independent of destination
validity, so check `endpoint_error` as well.

## Privacy and failure behavior

- `jcode telemetry disable`, `JCODE_NO_TELEMETRY=1`, and `DO_NOT_TRACK=1` still
  disable reporting. Transcript sharing remains separately opt-in.
- Consent is checked again before queued events or transcripts are delivered.
- When a custom destination is configured, **there is no fallback to the public
  service**, even if the receiver is down or the setting is invalid or empty.
- Redirects are not followed, so a receiver cannot redirect payloads elsewhere.
- Unset the variable to restore the public default. Do not set it to an empty
  string to restore defaults: empty explicitly disables delivery as invalid.
- Delivery remains best-effort with bounded queues, timeouts, and the existing
  usage-event retry policy. Permanent HTTP errors (most 4xx statuses) suppress
  usage-event delivery for the rest of the process. Restart after correcting a
  permanently rejected receiver configuration.

This setting changes **Jcode runtime** telemetry reporting only. It does not redirect model API
requests, discovery, updates, or other Jcode network traffic, and it does not
export local `--trace` logs.

The standalone shell installer's optional attribution request (used only when
`JCODE_INSTALL_CONVERSION_ID` is supplied) is separate and does not read this
setting. Set `JCODE_NO_TELEMETRY=1` for that installer process to suppress it.
