# Guided custom-agent creation

Date: 2026-09-10
Status: Design for user review. Generated instructions with manual entry approved in chat. Implementation not started.
Scope: Jcode TUI on the current `don` branch.

## 1. Outcome and approach

Add **Create new agent...** to `/agents` and a `/agents create` shortcut. A user can create and optionally activate a valid Markdown profile without knowing its file format or leaving the TUI.

The approved approach is a deterministic wizard with an optional AI drafting step. The user controls metadata and reviews the instructions before saving.

Alternatives considered:

- Manual-only form: simpler and works offline, but users must write the full instructions themselves. Keep this as a first-class path rather than the only path.
- Free-form conversation with a coding agent: flexible, but makes file paths, tool side effects, and save confirmation less predictable. Do not use a normal agent turn for generation.
- Guided wizard plus isolated text generation: recommended. Predictable state transitions and file writes, with editable instructions generated from a short description.

Existing integration points, verified from source:

- `/agents` dispatch and the native SSH guard: `crates/jcode-tui/src/tui/app/commands.rs:3179-3208`.
- Custom profile, clear, and built-in service rows: `crates/jcode-tui/src/tui/app/inline_interactive/openers.rs:12-148`.
- Profile fields and parsing: `crates/jcode-base/src/agent_profile.rs:29-143`.
- Global/project precedence: `crates/jcode-base/src/agent_profile.rs:145-205`.
- Skill resolution and activation: `crates/jcode-base/src/agent_profile.rs:221-290`.
- The provider interface accepts messages, tools, a system prompt, and an optional resume ID: `crates/jcode-provider-core/src/lib.rs:83-91`. Its simple-completion helper supplies one user message, no tools, and no resume ID at lines 466-491. This is the application-level isolation pattern, not proof about every external provider's internal behavior.

## 2. User flow

```text
/agents -> Create new agent...     /agents create
                     |                  |
                     +--------+---------+
                              v
                       Location and name
                              v
                       Purpose and usage
                              v
                    Generate or write myself
                              v
                    Review/edit instructions
                              v
                    Model, effort and skills
                              v
                       Final review
                         /         \
                     Save       Save and activate
```

### Location and name

Choose **Project** or **Global**. Default to Project when a session working directory is available. Otherwise default to Global and disable Project with an explanation. Show the full destination before saving. Use the session's working directory, not the shell's current directory, for project scope. Respect `JCODE_HOME` for global scope.

Require an explicit name. Apply existing ASCII letter/digit/hyphen/underscore validation, plus a wizard-only maximum of 64 characters. Do not silently rename, slugify, or overwrite. Reject an existing destination, including an invalid profile or symlink. A project profile shadowing a global profile is allowed only after the user acknowledges the visible warning. Warn when a name matches a built-in command or `create`, `use`, or `clear`, and show the unambiguous `/agents use <name>` invocation. Existing profiles named `create` remain selectable that way.

### Purpose and usage

Ask for a nonempty, single-line description, limited to 1,000 characters, and a purpose/context text area, limited to 4 KiB. The description becomes the picker description. Purpose/context is input to generation, not hidden metadata in the saved profile. For manual entry it can be omitted.

Choose **Main chat and workers** (`all`, default), **Main chat only** (`primary`), or **Workers only** (`subagent`). Explain the distinction without presenting it as a permission boundary.

### Instructions

Offer **Generate draft** and **Write myself**. Generation requires nonempty purpose/context. Show the generator's current provider/model before the request and explain that it uses the user's configured model connection. The generator uses the current session's provider/model snapshot, not the future agent's model selection.

Generated output goes into an editable multiline draft. It is never saved or activated automatically. The user can edit it, regenerate with confirmation before discarding edits, or switch to manual entry. Manual entry does not require provider credentials or network access.

Use the existing composer editing, paste, and newline behavior for text fields. Enter advances a completed text step. Escape returns to the preceding step. On the first step, Escape offers Keep editing or Discard; Keep editing is the default. While generating, Escape cancels generation and returns to the instruction choice. Draft instructions are limited to 64 KiB on both generation and save paths.

### Model, effort and skills

Model defaults to **Inherit**. Offer the current model catalog, preserving its explicit route identity. Selecting a model here changes the draft only, never the running chat. Effort defaults to **No effort override** and is omitted from the file unless selected. This leaves effort handling to the existing activation and provider behavior rather than promising a reset to a provider default. Display supported efforts from the selected route; for Inherit, use the current route and explain that later activation on another route can reject the chosen effort.

Skills are optional, searchable, and selected from the working-directory-aware skill registry. Store names, not skill contents, in the file. Do not invent or install missing skills. Revalidate selected skills at save time. Support skills-only profiles by allowing an empty instructions draft when at least one skill is selected.

### Review, save and activation

Show name, destination, description, usage mode, model/effort, skill names, and the scrollable instruction draft. Offer Edit, Save, Save and activate, and Cancel. Save is the default action. Workers-only profiles cannot select Save and activate. Display a concise warning that profiles are instruction/model presets, not permission sandboxes.

After saving, return to the refreshed agent picker with the created entry selected and show the exact saved path. Save alone preserves the current profile, provider, effort, and transcript. Save and activate uses the existing profile activation path and waits for its authoritative result. If activation fails, keep the successfully saved file, report the activation error, and leave the previously active profile unchanged. Do not misreport a saved file as unsaved or silently delete it.

## 3. Components and state boundaries

### TUI

Add a focused wizard controller under `crates/jcode-tui/src/tui/app/`. Keep a draft and explicit step enum in App state. Reuse existing picker rows, input handling, and rendering styles. Add a dedicated creation action rather than encoding it as a fake profile or model name. Avoid putting wizard logic into the already-large command dispatcher.

Both local and daemon-backed TUI key paths must route wizard input before ordinary chat submission. Opening, navigating, generating, cancelling, or saving must not add messages to the conversation or trigger a normal turn. Preserve any pre-existing composer draft. Reject entry during a running/pending turn or profile/model switch, and prevent chat submission while the wizard is open. Native SSH retains its existing local-configuration prohibition.

### Isolated generation

Use a shared application helper with one purpose: turn description, purpose/context, and usage mode into Markdown instruction text. Call the existing Provider streaming interface on an independent provider snapshot with a dedicated drafting system prompt, one user message, no tools, and no conversation resume ID. Do not include the transcript, active profile prompt, project files, skill bodies, or global assistant instructions. Do not start a swarm worker or a normal Agent turn. Do not silently switch to another model on errors.

Collect only text deltas. Reject tool-use events instead of dispatching them, reject empty text, abort above 64 KiB, and apply a 120-second total timeout. The system prompt requests instructions only, without YAML metadata or an outer code fence. A single outer Markdown code fence may be removed as presentation cleanup, but generated text cannot change metadata or destinations. Local generation returns through an asynchronous channel so input/rendering stays responsive.

For daemon-backed TUI, add dedicated GenerateAgentInstructions and CancelAgentInstructions requests plus an AgentInstructionsGenerated result carrying request ID, text or error, and generator model/provider identity. Bind jobs to the calling connection/session, permit one active generation per connection, and cancel on disconnect or session switch. The cancellation request identifies its generation request. Late responses are ignored after cancellation or after a draft changes. Reconnect preserves an in-memory draft but never silently retries generation. An older daemon rejecting the new request produces an upgrade/manual-entry message while keeping the draft.

Any provider route that cannot honor the no-tools, fresh-conversation completion contract must report generation unavailable and leave manual entry usable. Test the actual selected route through the real generation path before claiming provider-specific support. Tool omission is not a new OS sandbox.

### Persistence

Add a shared, narrowly scoped profile creation helper alongside the existing base profile parser. It receives an explicit scope, trusted working-directory anchor, validated name, and typed metadata plus body. Derive the path in code. Never accept a model-generated destination. Serialize YAML with a serializer, not string interpolation, then parse the complete result with the existing parser and resolve selected skills before writing.

Create the directory only on confirmed Save. Stage complete content in a temporary file in the destination directory, then publish it atomically with no-clobber semantics. An existing file, a symlink, or a concurrent creator must never be overwritten. Reject symlinked agent directories rather than writing outside the displayed scope. On I/O failure, remove only the temporary file owned by this operation and retain the draft for retry. Cancellation before Save leaves no profile file or directory behind. File updates and deletion are outside this feature.

Log generation start/completion/cancellation/failure and creation success/failure with request identity and duration where relevant. Do not log purpose text, generated instructions, or skill contents in the new logs. Existing provider logging policies remain unchanged. Track provider usage through existing accounting mechanisms without adding conversation messages.

## 4. Safety, errors and non-goals

The trust boundaries are user input, provider output, the filesystem, and asynchronous daemon responses. Validate names and limits before requests or writes. Treat generated content only as a reviewable instruction body. Keep all path, metadata, and activation decisions in deterministic application code.

Authentication/network/provider errors offer Retry or Write myself without losing the draft. Validation errors identify the field and return to its step. Save errors preserve inputs. Cancelling, reconnecting, or switching sessions must not apply stale results. Never expose credentials in error messages or new logs.

Out of scope: agent permissions/ACLs, OpenCode imports, automatic migration, profile editing/deletion, batch creation, third-party template catalogs, project scanning, new provider integrations, and a standalone CLI wizard. Do not modify original OpenCode configuration, existing custom profiles, or built-in service settings.

## 5. Acceptance map and delivery gate

The following checks are requirements for implementation. They are planned, not yet executed.

| ID | Requirement | Concrete check |
| :--- | :--- | :--- |
| W1 | Discoverable entry and shortcut | TUI tests and real tester: open `/agents`, select Create new agent, and separately invoke `/agents create`; both start the same first step. |
| W2 | Scope and naming | Temp-home/project tests verify exact global and project paths, missing-cwd behavior, invalid names, existing destinations, command collisions, and acknowledged cross-scope shadowing. |
| W3 | Valid Markdown metadata | Round-trip generated files through parse_profile and registry load using punctuation, quotes, multiline instructions, all modes, and optional model/effort/skills. |
| W4 | Manual/offline creation | Real TUI keyboard flow with provider requests instrumented to fail: write instructions, edit/back, save, and find the profile in the refreshed picker without any provider call. |
| W5 | AI drafting isolation | Recording provider fixture asserts one dedicated prompt, empty tools, absent resume ID, and no transcript/profile/skill bodies. Exercise text success, tool-use rejection, empty output, oversize output, timeout, and cancellation. |
| W6 | Real generation integration | Run the installed TUI and daemon on an isolated socket with the selected configured provider. Generate a draft, inspect/edit it, then save. Record actual response and resulting file. If credentials/network block this, report that limit and do not call this check passed. |
| W7 | Draft-only model and skill choices | Tests select a different route and effort plus skills, asserting the current provider/model/profile remain unchanged until explicit activation; reject missing skills and unsupported effort selections. |
| W8 | Safe writes | Test existing valid/invalid file, destination symlink, symlinked agent directory, concurrent creation, write/publish failure, and cancellation. Original bytes remain unchanged, no partial final file appears, and only owned temp files are removed. |
| W9 | Review and optional activation | Real manual creation followed by Save preserves the active session. Separate Save and activate succeeds and persists its instructions. Forced activation failure retains the new file and prior active profile. Workers-only mode disables activation. |
| W10 | Local/daemon parity and no chat side effects | Exercise both input paths through each step and assert unchanged message counts, no normal turn, preserved composer draft, and rejection while processing or switching models/profiles. |
| W11 | Async lifecycle and compatibility | Socket tests cover request correlation, stale response after cancel/edit, duplicate generation, session switch, disconnect/reconnect, older-daemon rejection, and retry without losing text. |
| W12 | Guardrails and regression | Native SSH rejects creation; built-in service picker settings and `/agents use create`, `/agents use clear`, and `/agents use review` keep their behavior. Existing custom-profile tests pass. |
| W13 | UI usability and delivery | Capture tester frames at narrow and normal widths, long errors and large pasted instructions. Check visible step title, focus, Back/Cancel hints, review scrolling, no clipping, and no unexpected chat turn. Build, install/reload, then repeat core manual and generated acceptance against the running commit. |

Keep a requirement-to-observation report with command/test name and observed result for each ID. Aggregate test counts are supplementary, not completion evidence. A successful build alone does not establish behavior. Do not change source implementation until the user has reviewed this specification.
