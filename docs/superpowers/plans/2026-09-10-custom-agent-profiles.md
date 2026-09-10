# Custom Agent Profiles Implementation Plan

**Goal:** Add discoverable Markdown agent profiles usable in the current conversation and swarm workers without rebuilding Jcode for each new profile.

**Architecture:** A shared profile loader in jcode-base reads global and project agent files. Runtime activation resolves skills, validates model and effort on an isolated provider, and persists the active prompt snapshot with the session. The TUI lists profiles separately from built-in service settings and sends an explicit activation request. Swarm spawn accepts a profile name and resolves it against the worker workspace.

**Tech Stack:** Rust, existing serde YAML parser, existing session JSON persistence, existing client/server protocol and TUI picker.

## Approved behavior

- Global ~/.jcode/agents/*.md and project .jcode/agents/*.md, project names override global names.
- Frontmatter: description, mode (primary/subagent/all), model (inherit or configured model), effort, skills. Filename is the name. Markdown body is the instruction prompt.
- Unknown fields, especially permission/tool restrictions, produce explicit errors instead of being ignored.
- Existing service settings remain available. Current-chat activation respects mode; worker activation respects mode.
- Selection preserves transcript and appends profile instructions to the normal system prompt rather than replacing safety/project instructions.
- Persist resolved instructions so reload/resume does not silently change or lose the profile. Reload files at discovery/activation time. Clearing a profile clears instructions but keeps the current model.
- No modification of original OpenCode files and no implicit migration of user profiles.

## Execution

1. Profile loader: add tests for parsing, invalid fields, invalid modes/effort, name traversal, CRLF, sorted discovery, project overrides including invalid overrides, missing directories and reload-after-edit. Implement the loader in crates/jcode-base/src/agent_profile.rs and export it from lib.rs.
2. Session/runtime: add persisted active profile snapshot; test old-session compatibility and snapshot restoration. Add Agent activation that resolves every skill before mutation, validates provider configuration on a fork, saves once, and logs name only. Append snapshot prompt in agent/prompting.rs. Add SetAgentProfile request and AgentProfileChanged response with protocol serialization coverage.
3. TUI: extend /agents discovery and direct selection, retain built-in service targets, reject selection during active turns and SSH local configuration, update picker actions and response handling, and add picker/command tests.
4. Workers: accept profile in swarm spawn schema and wire request, resolve before creating a worker, use explicit model/effort overrides over profile defaults, and inject the resolved profile before its initial turn. Test mode restrictions and field forwarding.
5. Validation: run focused tests for base profiles, protocol, app-core activation/spawn and TUI picker; format changed Rust files; coordinated TUI build; use debug tester against the resulting build to exercise /agents and selection. Commit only this feature and reload the validated binary.

## Acceptance checks

A newly created profile appears on reopening /agents. Project definitions override global ones. Selecting a profile applies its model and skill instructions without invoking an LLM turn. Invalid profile/model/skill selections leave the previous active profile unchanged. A worker spawned with profile uses its instructions on its first turn. Existing service model settings still work. Old sessions still load, selected profiles survive resume, and unsupported permission fields cannot be mistaken for enforced restrictions.
