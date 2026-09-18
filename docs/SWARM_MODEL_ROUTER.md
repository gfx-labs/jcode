# Task-aware swarm model routing with TypeSafe Jev

Jcode can ask Jev to select a model for each **new** swarm worker. The assigned
initial task is the state and available model routes are the Choice options.
This is opt-in and does not change the coordinator's model or existing workers.

## Configuration

Export `TYPESAFE_API_KEY` in the environment that starts the Jcode daemon.
For background daemons, the standard private credential file
`~/.config/jcode/typesafe.env` can instead contain `TYPESAFE_API_KEY=...`.
Keep that file mode `0600`. The environment takes precedence. Never put the key
in the routing config, a task prompt, or a repository.

Add this to `~/.jcode/config.toml` and restart or gracefully reload the daemon:

```toml
[agents.swarm_router]
enabled = true
model = "jev-latest"
timeout_ms = 5000
candidates = [
  "claude-oauth:claude-haiku-4-5-20251001",
  "claude-oauth:claude-sonnet-5",
  "openai-oauth:gpt-5.6-sol",
  "openai-oauth:gpt-6-astra",
]

[agents.swarm_router.descriptions]
"claude-oauth:claude-haiku-4-5-20251001" = "Preferred for bounded lookup, bulk reading, fact extraction, and summarization with little reasoning."
"claude-oauth:claude-sonnet-5" = "Preferred for scoped implementation, UI work, and routine bug fixes with clear acceptance criteria."
"openai-oauth:gpt-5.6-sol" = "Preferred for substantial coding, multi-file implementation, refactoring, and writing or extending tests."
"openai-oauth:gpt-6-astra" = "Preferred for architecture, ambiguous investigation, hard debugging, security-sensitive review, and complex verification."
```

These descriptions are an editable **local assignment policy**, not benchmark
claims about the models. Use `swarm list_models` to see routes available to your
account. Only available routes can be selected. An empty `candidates` list uses
the available catalog. A curated shortlist and clear descriptions make the
routing decision more useful than a large set of unexplained model names.

## Precedence and fallback

1. An explicit worker `model`, including `inherit`, bypasses Jev.
2. When enabled and given a nonempty task, Jev selects from the available candidates.
3. If routing cannot run or fails, Jcode uses the existing `agents.swarm_model`.
4. Without that default, the worker inherits the coordinator's model and route.

Omit `model` on `spawn`, `assign_task`, `assign_next`, `fill_slots`, and `run_plan`
to allow routing when those actions create a worker. Reusing a worker does not
change its model. Reasoning `effort` remains independent.

Routing uses `POST https://api.typesafe.ai/v1/systemone` with a typed Choice
question. It sends the assigned initial task, not the full conversation. Task
text may contain project information, so enable this only when sending that
information to TypeSafe is appropriate. No task text, credentials, or raw API
responses are written to routing logs. The network call is bounded by a timeout
and an invalid or unavailable model cannot be used as a selection.

To disable routing, set `enabled = false`. Your previous model default remains
in place. A shell newly sourcing a key does not update a running daemon's
environment, and a graceful daemon reload inherits the old environment. Use the
private credential file or start the daemon from the updated environment.

API documentation: <https://docs.typesafe.ai/api.md>
Choice guidance: <https://docs.typesafe.ai/primitives/choice.md>
