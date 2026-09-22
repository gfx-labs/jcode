# Jev providers and shared transport

Skill suggestions, swarm model routing, browser handoff, and memory recall all
use the upstream `JevClient` typed Decisions transport, not chat completions.
They do not all expose the same provider-selection knobs.

## Shared hosted configuration

Skill suggestions, swarm routing, and browser handoff by default use
`[agents.jev]` in `~/.jcode/config.toml`. Supported shared providers are
`typesafe` (default) and `openrouter`. Both require an API key.
Local provider `openjev` is rejected, with no silent hosted fallback.

Add or update this table without duplicating it:

```toml
[agents.jev]
provider = "typesafe"
# Optional hosted overrides:
# base_url = "https://api.typesafe.ai/v1"
# model = "jev-latest"
# api_key_env = "TYPESAFE_API_KEY"
# timeout_ms = 5000
```

TypeSafe defaults to `https://api.typesafe.ai/v1`, model `jev-latest`, and
`TYPESAFE_API_KEY` from the daemon environment or
`~/.config/jcode/typesafe.env`. OpenRouter uses its Decisions API, model
`typesafe/jev-1.13`, and `OPENROUTER_API_KEY`/`openrouter.env`.
Keep credential files private (mode `0600`) and credentials out of TOML and
version control. Missing credentials fail the decision rather than selecting
another account. Endpoint, model, credential-variable, and timeout overrides
remain supported for these hosted routes. Timeouts are bounded to 1..=30000 ms.
Remove stale overrides when switching providers.

Enable consumers separately, retaining your existing settings:

```toml
[agents]
skill_suggestion_backend = "jev"

[agents.swarm_router]
enabled = true
```

`JCODE_JEV_PROVIDER` overrides the shared provider. Browser handoff also retains
its explicit `JCODE_BROWSER_JEV_PROVIDER` selector for `auto`, `jcode`,
`openrouter`, `typesafe`, or `aimlapi`. That selector uses the upstream route's
endpoint/model rather than shared overrides. See [browser handoff](BROWSER_FAST_AGENT.md).

Memory remains independent: `agents.memory_jev_provider` (default `auto`) and
`JCODE_MEMORY_JEV_PROVIDER` select `auto`, `jcode`, `openrouter`, `typesafe`, or
`aimlapi`. Auto prefers Jcode, then OpenRouter, TypeSafe, and AI/ML API credentials.
Jcode subscription routes require live purpose-specific gateway entitlement.
Using the same client does not make `[agents.jev]` control memory, nor enable
arbitrary raw gateway choices for every consumer.

## Reload, precedence, and failure behavior

After installing a changed binary, load it into the daemon once. Thereafter,
config-file changes apply to new decisions through config metadata checks
throttled to roughly 500 ms, without a daemon restart. An in-progress browser
handoff retains its starting transport. Existing swarm workers retain their
models. Environment changes require starting the server with the new environment.

Legacy explicit skill endpoint/model/key settings remain consumer-specific
overrides. Non-default swarm model and timeout values override shared settings;
legacy defaults (`jev-latest` and 5000 ms) inherit shared settings. Shared provider
selection alone does not enable skill suggestions or swarm routing.

A failed skill decision produces no suggestion. A failed swarm decision uses
existing default/inheritance rules. A failed browser decision returns control.
These are application fallbacks, not silent requests to a different provider.
Memory retains its independent selector and failure behavior through `JevClient`.
