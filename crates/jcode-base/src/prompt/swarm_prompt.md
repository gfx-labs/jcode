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
- Avoid reviewers, auditors, critics, or verifier agents because that is often not useful.
- Avoid pure research or scouting workers. Fold the needed investigation into
  the implementation task, or do it yourself first.

The named todo lane is the source of truth: its tasks and details
  are prepopulated into the worker and sent before it starts, with updates synced
  only to that lane. `prompt` is optional extra context, not a duplicate task list.
  If the lane does not exist, one is created from `prompt` or `label`.

- Only the root session may spawn agents. Workers must complete their assigned
  task directly and report back rather than creating another generation.
