# Per-turn skill suggestion with TypeSafe Jev

Opt-in per-turn routing that asks a TypeSafe System One decision model
(Jev) which registered skill, if any, best fits the user's latest request,
and injects that skill's prompt automatically when the model is confident.

This is separate from [swarm model routing](SWARM_MODEL_ROUTER.md), which
routes *spawned agent models*, not skills.

## Enabling

```toml
[agents]
skill_suggestion_backend = "jev"          # default: "off"
# skill_suggestion_model = "jev-latest"           # default
# skill_suggestion_base_url = "https://api.typesafe.ai/v1"  # default
# skill_suggestion_api_key_env = "TYPESAFE_API_KEY"          # default
# skill_suggestion_min_confidence = 0.6                       # default
```

Requires a TypeSafe API key, resolved the same way as other TypeSafe-backed
features in jcode:

1. The environment variable named by `skill_suggestion_api_key_env`
   (default `TYPESAFE_API_KEY`) in the server process environment, or
2. `~/.config/jcode/typesafe.env` (mode `0600`), via the existing
   provider-catalog config-file loader
   (`crate::provider_catalog::load_api_key_from_env_or_config`). This is the
   same file/loader used by [swarm model routing](SWARM_MODEL_ROUTER.md), so a
   key saved for that feature also covers skill suggestion.

If no key is found, the feature does nothing (fails open) regardless of the
`skill_suggestion_backend` setting.

## Endpoint

Requests go directly to TypeSafe's System One endpoint, not through
OpenRouter:

```
POST {skill_suggestion_base_url}/systemone
```

There is no separate `/decisions` path; `/systemone` is the only endpoint
used.

with `Authorization: Bearer {api_key}` and a `choice`-type question whose
criteria are the registered skills' names and descriptions, plus an always
offered `none` option so the model can abstain.

## What is sent, and when

- Nothing is sent unless `agents.skill_suggestion_backend = "jev"` is set.
- Each request's `state` is a short tail of the current conversation: at
  most the last few trailing user/assistant turns, text content only (tool
  calls and results are excluded), each truncated to a bounded character
  cap.
- The `criteria` are the currently registered skills' names and short
  descriptions (the same descriptions already shown to the agent for
  ordinary skill invocation), so the model can pick a skill without seeing
  its full contents.
- No suggestion from a previous turn is reused or carried forward; every
  fresh user turn's request is independent and reflects only that turn's
  conversation tail.

## When the router runs, and what happens after

Every fresh user turn performs a bounded async wait for a decision
*before* the provider request for that turn is sent, up to an internal
~1500ms timeout:

- The router is invoked and the turn waits for an answer before the
  provider call is made. If it answers in time and clears
  `skill_suggestion_min_confidence`, the winning skill is used for that
  turn. In practice Jev typically answers well under the timeout (observed:
  `pdf` at confidence 0.88 in 284ms).
- If the wait times out, the request is dropped and the turn proceeds
  without a suggestion. There is no background continuation, no
  carry-over, and no TTL-based cache: a timed-out or unused decision is
  simply discarded, not consumed on a later turn.
- Timeouts, missing credentials, and any HTTP or parse failure all fail
  open: no suggestion is injected, and the turn proceeds exactly as it
  would with the feature off.

## What gets injected

When a skill is accepted, only that one skill's full prompt/content is
fetched and injected into the current turn's context. It is ephemeral: it
is not written back into config or persisted session state, and it does not
change what is sent on the next turn beyond what that skill's own
invocation would already add.

## Configuration reference

| Key | Default | Meaning |
| --- | --- | --- |
| `agents.skill_suggestion_backend` | `"off"` | `"off"` or `"jev"`. Env: `JCODE_SKILL_SUGGESTION_BACKEND` |
| `agents.skill_suggestion_model` | `"jev-latest"` | Decision model slug. Env: `JCODE_SKILL_SUGGESTION_MODEL` |
| `agents.skill_suggestion_base_url` | `"https://api.typesafe.ai/v1"` | No trailing slash. Env: `JCODE_SKILL_SUGGESTION_BASE_URL` |
| `agents.skill_suggestion_api_key_env` | `"TYPESAFE_API_KEY"` | Env var name holding the bearer key. Env: `JCODE_SKILL_SUGGESTION_API_KEY_ENV` |
| `agents.skill_suggestion_min_confidence` | `0.6` | Minimum confidence (0.0-1.0) to accept a suggestion. Env: `JCODE_SKILL_SUGGESTION_MIN_CONFIDENCE` |

All settings have safe built-in defaults; only `skill_suggestion_backend`
needs to change to opt in.
