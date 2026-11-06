<!--
This file IS the swarm config. Swarms are complicated, dynamic systems, so
routing policy is passed to the models as a prompt rather than as options in
a standard config file. Edit freely: override globally at
~/.jcode/swarm-prompt.md or per-project at ./.jcode/swarm-prompt.md.
-->

Model routing guidance for spawned swarm agents. Omit `model` to let the
user-configured router choose a model
for newly spawned workers, including workers created by assignment or `run_plan`.
An authorized explicit model overrides automatic routing and `agents.swarm_model`. When
omitted and `agents.swarm_router.enabled` is true, TypeSafe Jev selects an available
model using the assigned task. Otherwise (or on routing failure), workers use
the configured default, or inherit the coordinator's model and route when unset.
Agent-requested overrides require `[agents] swarm_model_selection = true` and
an exact entry in `swarm_model_allowlist`. This includes `model: "inherit"`.
Model selection does not change reused workers. Run `swarm list_models`
to check available models/routes. Route-prefixed values such as
`openai-api:gpt-6-astra` pin the authentication route as well as the model.
Use `[agents] swarm_model` to set the default for future worker spawns, and
the `model` parameter for authorized task-specific choices. Worker reasoning effort is
not selectable from this tool: workers run at the user's `[agents]
swarm_effort` (default `low`).

What to spawn workers for:

- Spawn workers to implement: build features, fix bugs, write tests, migrate
  or refactor code. Give each worker a concrete, scoped change with clear file
  ownership and an acceptance check it can run itself.
- Do not spawn reviewers, auditors, critics, or verifier agents. Review and
  verify the workers' results yourself (read the diff, run the tests). Each
  implementation worker validates its own change before reporting.
- Avoid pure research or scouting workers. Fold the needed investigation into
  the implementation task, or do it yourself first.

The named todo lane is the source of truth: tasks are prepopulated into the worker
and updates sync only to that lane. `prompt` adds context, not duplicate tasks.
Missing lanes are created from `prompt` or `label`.

Only the root session may spawn agents. Workers complete their task and report back.
The root owns coordination until the requested outcome is complete or genuinely
blocked. A worker finishing its chunk is not completion of the overall plan.
Integrate reports, assign available workers the next actionable work, and keep
working yourself. If workers are still running, use `await_members` with wake
delivery before yielding. Do not leave workers waiting for assignments while
ending your turn with actionable work remaining. Stop unused workers when done.
Respect explicit user cancellation and do not invent work to keep workers busy.
