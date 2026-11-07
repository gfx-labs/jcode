# Jcode mobile app

The current iPhone client lives in the separate
[jcode-mobile repository](https://github.com/1jehuang/jcode-mobile).
That repository owns the Rust/GPUI app, iOS platform integrations, mobile tests,
build scripts, and App Store metadata. See its README for development and
platform setup instructions.

The legacy Swift/SwiftUI client formerly in this repository's `ios/` directory
and its `ios-testflight.yml` workflow were retired on November 6, 2026. Their
sources, tests, and release documentation remain available in Git history.
This removal does not assert App Store release readiness for the replacement.

## What stays in this repository

Mobile clients still depend on Jcode's server and shared libraries. Keep the
pairing and gateway endpoints, harness API, wire protocol, cloud session sync,
mobile push support, and `jcode-sdk` here. The separate mobile repository
consumes the SDK from this repository, not the retired Swift client.

Server-side mobile support is not deprecated by this cleanup. Mobile UI and
client release work should happen in `jcode-mobile`, while backend and SDK
changes should continue here.

## Gateway listener and advertised host

Configure `~/.jcode/config.toml` (or `$JCODE_HOME/config.toml`):

```toml
[gateway]
enabled = true
bind_addr = "127.0.0.1"
port = 7643
connect_host = "my-machine.example.ts.net"
```

`bind_addr` controls the listener. `connect_host` is the hostname or IP advertised
by `jcode pair` and `/remote` status/pairing, without a scheme or port. Setting it
does not change the listener or make it remotely reachable. Keep loopback for a
tunnel/proxy, or bind to a reachable interface for direct access. Wildcard binds
listen on all interfaces, so restrict access with a firewall or private network.

Host precedence is nonblank `JCODE_GATEWAY_HOST`, nonblank `connect_host`, a
specific `bind_addr`, then Tailscale MagicDNS and the system hostname. Blank
overrides are ignored. For example, this works even with a loopback listener:

```sh
JCODE_GATEWAY_HOST=proxy.example.test jcode pair
```

The port still comes from `gateway.port`; this setting does not configure a
separate proxy port or TLS scheme. The CLI reads configuration for each invocation.
Listener changes require a server reload. Environment changes for daemon-side
commands require restarting the daemon with that environment.
