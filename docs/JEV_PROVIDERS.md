# Jev providers: hosted TypeSafe or local Open-Jev

Jcode's skill suggestions, task-aware swarm model routing, and fast browser
handoff share `[agents.jev]` in `~/.jcode/config.toml`. The default is hosted
TypeSafe. Open-Jev uses the same typed decision protocol, not chat completions.

## Switch to local Open-Jev

Start your Open-Jev server, then add or update this table (do not duplicate it):

```toml
[agents.jev]
provider = "openjev"
# base_url = "http://127.0.0.1:8791/v1"
# model = "jev-latest"
# timeout_ms = 15000
```

No TypeSafe or OpenRouter key is needed or implicitly sent in local mode.
`base_url` is the API base, not the complete `/systemone` path. An optional
`api_key_env` explicitly opts into a separate key for an authenticated local
proxy. Requests are not redirected. Selecting local mode never falls back to a
hosted Jev endpoint if the local service is unavailable.

Enable the consumers you want separately, retaining your existing settings:

```toml
[agents]
skill_suggestion_backend = "jev"

[agents.swarm_router]
enabled = true
```

Browser `handoff` uses the selected provider when invoked. Its action validation,
confidence gate, explicit tab scope and hand-back behavior remain unchanged.

## Switch back to hosted TypeSafe

```toml
[agents.jev]
provider = "typesafe"
```

Remove local `base_url`, `model`, and `api_key_env` overrides when switching
providers unless you deliberately want to retain them. Hosted defaults are
`https://api.typesafe.ai/v1`, `jev-latest`, and `TYPESAFE_API_KEY` from the daemon
environment or `~/.config/jcode/typesafe.env`. Keep credentials out of TOML and
version control. A missing hosted key fails closed for the request, not into a
local or alternate provider.

To retain the previous browser OpenRouter transport instead, select
`provider = "openrouter"`. It uses OpenRouter's Decisions API, model
`typesafe/jev-1.13`, and `OPENROUTER_API_KEY`/`openrouter.env`. This selection applies
to all three Jev consumers, not just browser handoff.

## Reload and precedence

After installing the updated binary, load it into the daemon once. Thereafter,
config-file provider changes are picked up for new decisions via Jcode's config
cache (metadata checks are throttled to roughly 500 ms), without restarting the
daemon. An in-progress browser handoff retains the transport it started with.
Existing swarm workers retain their models. Environment changes still require
starting the server with the new environment.

Legacy explicit skill-suggestion endpoint/model/key settings are preserved as
consumer-specific overrides. Non-default swarm model and timeout values override
shared settings; legacy defaults (`jev-latest` and 5000 ms) inherit shared settings. Remove stale overrides
when adopting a shared provider selection. Global provider selection alone does
not enable skill suggestions or swarm routing.

Local requests default to a longer 15-second budget because local candidate
scoring can be slower for large skill catalogs. Request deadlines remain bounded.
A failed skill decision produces no suggestion; a failed swarm decision uses the
existing default/inheritance rules; a failed browser decision returns control.
None of those fallbacks secretly sends a request to hosted Jev.

## Local model limits and cache

The local 2B server is independently trained and is not guaranteed to match
hosted Jev quality or confidence calibration. Its configured input-token limit
can reject large browser pages or conversation states. Keep candidate lists
bounded. Do not lower browser safety thresholds just to force a local action.

Prefix caching is a server-side Open-Jev setting, not a Jcode response cache.
Inspect `metadata.prefix_cache` from a raw `/v1/systemone` response to verify
actual reuse. Upstream marks CUDA prefix caching experimental because output
probabilities can differ. Compare cached/uncached predictions on your hardware
before enabling `--prefix-cache`; `--no-prefix-cache` is the safe upstream default.

## Cross-request context reuse

Open-Jev prefix caches are request-local: they reduce repeated prefix computation
across candidates in one call, then are discarded. They do not provide a saved
context ID for later routing calls. TypeSafe's public API also requires `state`
and `questions` on each call and documents no persistent context handle. Reusing
a stored routing template would require an application-side extension; that alone
would save payload bytes, not establish GPU prefill reuse across requests.
