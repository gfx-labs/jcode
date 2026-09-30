# Task-aware swarm model routing with Jev

Jcode can ask Jev to select a model for each **new** swarm worker. The assigned
initial task is the decision state and the configured candidate routes are the
options of one `choice` question. The coordinator and reused workers are not
changed.

## Transport

Routing uses the shared `JevClient` with its own purpose (`swarm`), like memory,
browser, and skill routing. Provider and credentials follow
`JCODE_SWARM_JEV_PROVIDER` (default `auto`: Jcode subscription, then TypeSafe,
OpenRouter, AI/ML API). See the Jev provider docs for credential files.

## Configuration

```toml
[agents.swarm_router]
enabled = true
timeout_ms = 5000
candidates = [
  "openai-oauth:gpt-6.1-sol",
  "claude-oauth:claude-opus-5-5",
  "openai-oauth:gpt-6-luna",
  "zai:glm-5.3",
]

[agents.swarm_router.efforts]
"openai-oauth:gpt-6.1-sol" = "high"
"claude-oauth:claude-opus-5-5" = "medium"

[agents.swarm_router.service_tiers]
"openai-oauth:gpt-6-luna" = "priority"

[agents.swarm_router.descriptions]
"openai-oauth:gpt-6.1-sol" = "Choose for hard debugging, architecture, and verification."
"zai:glm-5.3" = "Choose for reading, searching, and summarizing."
```

- `candidates` are route-pinned specs (`openai-oauth:gpt-6-luna`,
  `zai:glm-5.3`) or bare model ids. Only routes that are currently available
  are offered. An empty list offers every available route.
- `descriptions` are the policy Jev evaluates the task against. They are local
  assignment policy, not benchmark claims. Missing entries fall back to catalog
  text, so write one for every candidate.
- `efforts` sets the worker's reasoning effort when that candidate wins.
- `service_tiers` sets the OpenAI service tier (`priority`/`fast`, `flex`,
  `off`) when that candidate wins. It overrides
  `agents.swarm_openai_service_tier` for that worker and has no effect on
  non-OpenAI routes.
- Keys in `descriptions` and `efforts` match the candidate as written.

Config changes apply to the next spawn without a restart.

## Precedence and fallback

1. An explicit worker `model` (including `inherit`) bypasses routing. An
   explicit `effort` overrides the routed effort.
2. When enabled with a nonempty task, Jev picks from the available candidates.
   A single available candidate is used without a request.
3. On missing credentials, timeout, or an answer outside the offered set, Jcode
   uses `agents.swarm_model` and `agents.swarm_effort`.
4. Without those, the worker inherits the coordinator's model and route.

## What Jev sees

The decision state is a JSON object built only from the worker's assignment,
never the coordinator conversation:

- `task`: the spawn prompt, or for `assign_next`/`run_plan` spawns the plan item
  content plus any coordinator message (capped at 8000 characters)
- `label`: the spawn label, when given
- `node_kind`: plan node kind (explore, implement, verify, fix, critique,
  synthesize)
- `subsystem`, `file_scope` (up to 20 paths), `swarm_mode` (deep or light)

Empty fields are omitted. Credentials and raw responses are not logged. The
selection and confidence are logged.
