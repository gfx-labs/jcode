# Custom agents

Add a Markdown file to `~/.jcode/agents/` for a global agent, or `.jcode/agents/` in a project for a project-specific agent. The filename is the agent name. Files are read again whenever you open the picker or activate a profile. No rebuild is needed after adding or editing an agent file.

For example, save this as `~/.jcode/agents/reviewer.md`:

```markdown
---
description: Review changes for correctness and missing tests
mode: all
model: inherit
---

Review the current changes. Report concrete correctness, security, and test gaps.
Do not edit files unless the user asks for fixes.
```

Open `/agents` and select `reviewer`, or enter `/agents reviewer`. Selection changes the active profile without sending a message to the model. It preserves the conversation transcript.

## Profile fields

| Field | Meaning |
|-------|---------|
| `description` | Required nonempty picker description. |
| `mode` | `primary`, `subagent`, or `all` (default). Primary profiles can be selected in the current chat. Subagent profiles can be used for workers. |
| `model` | A Jcode model or route-pinned model spec. Omit it or use `inherit` to keep the current model when selecting a profile. |
| `effort` | Optional reasoning effort: `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, or `max`. The selected provider must support the value. |
| `skills` | Optional YAML list of existing skill names. Their instructions are resolved and included in the profile prompt at activation. |

Names may contain ASCII letters, digits, hyphens, and underscores. The body supplies instructions. A profile needs a nonempty body or at least one skill.

A profile using a migrated skill can be short:

```markdown
---
description: Review using the team's QA workflow
mode: all
model: inherit
skills:
  - review-qa
---
```

Project profiles override global profiles with the same name. An invalid project override produces an error, not a fallback to the global definition. Unknown fields, missing skills, invalid modes, and unsupported model or effort selections are reported rather than silently ignored.

## Built-in services and clearing

The existing service model settings remain under `/agents swarm`, `/agents review`, `/agents judge`, `/agents memory`, and `/agents ambient`. If a custom agent has a conflicting name, select it explicitly with `/agents use review` or through its custom picker entry.

`/agents clear` removes the active profile instructions. It keeps the current model and effort. Use `/agents use clear` to select a profile named `clear`.

The resolved profile instructions are saved with the session. Resume and daemon reload retain the selected instructions even if the source file changes or disappears. Select the profile again to apply an edited definition. Changing profiles while a turn is running is rejected.

## Swarm workers

Ask the coordinator to spawn a worker using a profile. The native tool request is:

```json
{
  "action": "spawn",
  "profile": "reviewer",
  "label": "Review current changes",
  "prompt": "Review the current diff and report findings."
}
```

The worker profile is resolved in its working directory. Its instructions and skills are installed before the first turn, including visible workers. Explicit `model` and `effort` arguments override profile defaults. When a profile omits a model, normal swarm defaults apply; `model: inherit` explicitly requests the coordinator's model.

## Permissions and compatibility

Agent profiles are instruction and model presets, not permission sandboxes. A sentence such as “do not edit files” is an instruction, not an enforced tool restriction. The normal global safety checks still apply.

`permission`, `permissions`, `tools`, and other unsupported frontmatter fields are rejected. Do not copy an OpenCode permission profile and assume the restrictions are enforced. Original OpenCode files are never imported or modified automatically.

Agent configuration through `/agents` remains unavailable in native SSH mode, matching the existing local-configuration guard.
