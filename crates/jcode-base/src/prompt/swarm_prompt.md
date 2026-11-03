<!--
This file IS the swarm config. Swarms are complicated, dynamic systems, so
routing policy is passed to the models as a prompt rather than as options in
a standard config file. Edit freely: override globally at
~/.jcode/swarm-prompt.md or per-project at ./.jcode/swarm-prompt.md.
-->

Model selection policy is supplied in global JCODE.md. Use `jev_decision` when
that policy calls for a model choice, then pass the selected `model`, `effort`,
and applicable `service_tier` explicitly. Run `swarm list_models` to discover
available routes. There is no automatic swarm router. Omitted values use the
configured defaults; `model: "inherit"` selects the coordinator's model.

- Spawn workers when work can be parallelized.
- Avoid reviewers, auditors, critics, verifiers, and research-only scouts.
  Include investigation in implementation work or do it yourself.

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
