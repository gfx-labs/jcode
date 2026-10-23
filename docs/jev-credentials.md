# Jev credentials

The shared Jev client supports an optional block in `~/.jcode/config.toml`:

```toml
[jev]
provider = "openrouter"
# Optional inline override. Prefer the private env file below.
openrouter_api_key = ""
```

Purpose-specific provider environment variables still override `jev.provider`.
Voice retains its upstream provider restrictions and does not use this override.

For a Jev-only OpenRouter key, put this in `~/.config/jcode/jev.env`:

```dotenv
JCODE_JEV_OPENROUTER_API_KEY=your-key
```

Set permissions to `0600`. With `JCODE_HOME` set, the env file is
`$JCODE_HOME/config/jcode/jev.env`.

A nonblank inline key takes precedence, followed by the dedicated Jev key,
then upstream `OPENROUTER_API_KEY` / `openrouter.env` lookup. These overrides
apply only to Jev, not chat provider authentication. Keys are redacted from
configuration Debug output. The shared client loads configuration and keys
when constructing each new decision client.
