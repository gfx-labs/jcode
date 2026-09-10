# Guided Agent Creation Implementation Plan

> **For agentic workers:** Use `subagent-driven-development` to execute these tasks. Steps use checkboxes for tracking. Load the assigned role and `test-driven-development` before changing code. Do not delegate recursively.

**Goal:** Create editable Markdown agent profiles through `/agents`, using manual instructions or an isolated AI draft, without altering the active conversation until explicit activation.

**Architecture:** The TUI owns the draft and confirmation state. A base-layer helper owns validated, no-clobber profile persistence. A separate completion helper and correlated daemon requests own optional generation. Existing profile activation remains authoritative.

**Tech Stack:** Rust, Tokio, serde_yaml, tempfile, the existing Provider stream interface, ratatui/crossterm TUI, Jcode protocol and selfdev tools.

**Approved design:** `docs/superpowers/specs/2026-09-10-agent-creation-wizard-design.md`. User approved the written specification at 2026-09-10 21:11 UTC. Work stays on `don`, without a new worktree, push or PR.

## Files and ownership

| Task | Files | Owner |
| :--- | :--- | :--- |
| Profile persistence | `crates/jcode-base/src/agent_profile.rs`, new `crates/jcode-base/src/agent_profile/create.rs` | Coordinator |
| Generation | new `crates/jcode-base/src/agent_instructions.rs`, `crates/jcode-base/src/lib.rs`, `crates/jcode-protocol/src/{wire,lib}.rs`, `crates/jcode-app-core/src/server/{mod,client_lifecycle}.rs`, new `crates/jcode-app-core/src/server/agent_instructions.rs` | Generation worker |
| Wizard | `crates/jcode-tui/src/tui/app/{agent_wizard.rs,agent_wizard_tests.rs}`, App fields/initializers, command dispatch, picker action/openers, local/remote input, polling/event handling, TUI rendering and help | TUI worker |
| Integration | `docs/CUSTOM_AGENTS.md`, this plan and the design status, focused tests, owned scratch acceptance artifacts | Coordinator |

Workers must avoid files outside their ownership and request coordination for shared files. All Cargo operations use the coordinated `selfdev test` or `selfdev build` tools. Set `PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`. Do not overlap uncoordinated builds. Start with tests using existing public inputs wherever possible, and report observed RED before adding production behavior.

## Shared interfaces

The persistence module exports these through `crate::agent_profile`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentProfileScope { Global, Project }

#[derive(Debug, Clone)]
pub struct AgentProfileDraft {
    pub name: String,
    pub description: String,
    pub mode: AgentMode,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub skills: Vec<String>,
    pub prompt: String,
}

pub fn profile_destination(
    scope: AgentProfileScope,
    working_dir: Option<&std::path::Path>,
    name: &str,
) -> anyhow::Result<std::path::PathBuf>;

pub fn create_profile(
    scope: AgentProfileScope,
    working_dir: Option<&std::path::Path>,
    draft: &AgentProfileDraft,
) -> anyhow::Result<AgentProfile>;
```

The generation module exports:

```rust
#[derive(Debug, Clone)]
pub struct GeneratedAgentInstructions {
    pub text: String,
    pub model: String,
    pub provider_name: String,
}

pub async fn generate_agent_instructions(
    provider: std::sync::Arc<dyn crate::provider::Provider>,
    description: String,
    purpose: String,
    mode: String,
) -> anyhow::Result<GeneratedAgentInstructions>;
```

Wire additions use existing serde conventions:

```rust
GenerateAgentInstructions {
    id: u64,
    description: String,
    purpose: String,
    mode: String,
},
CancelAgentInstructions { id: u64, generation_id: u64 },

AgentInstructionsGenerated {
    id: u64,
    text: Option<String>,
    model: String,
    provider_name: String,
    error: Option<String>,
},
```

The generator accepts an active provider handle but forks internally before completion. The TUI must not mutate its provider for draft choices. In the daemon the provider snapshot is acquired under a short Agent lock, then all network work happens without that lock. Cancellation aborts the connection-owned generation job. No normal turn or transcript event is produced.

## Task 1: Validated no-clobber profile creation

Covers W2, W3, W8. Reuse `AgentMode`, `parse_profile`, registry skill resolution and `tempfile`, already declared in jcode-base.

- [ ] Add tests for the public writer contract. Include this round trip and duplicate-write assertion, using the existing isolated JCODE_HOME test guard:

```rust
let draft = AgentProfileDraft {
    name: "wizard-test".into(),
    description: "Review: \"quoted\" YAML # safely".into(),
    mode: AgentMode::All,
    model: None,
    effort: None,
    skills: vec![],
    prompt: "Review code.\nReport concrete issues.".into(),
};
let profile = create_profile(AgentProfileScope::Project, Some(root.path()), &draft)?;
assert_eq!(profile.path, root.path().join(".jcode/agents/wizard-test.md"));
let original = std::fs::read(&profile.path)?;
assert_eq!(profile.description, draft.description);
assert!(create_profile(AgentProfileScope::Project, Some(root.path()), &draft).is_err());
assert_eq!(std::fs::read(&profile.path)?, original);
```

- [ ] Run `cargo test -p jcode-base --lib agent_profile_create` via selfdev and record the missing-behavior failure. Add a minimal callable failing implementation only if needed to turn an unresolved symbol into an assertion failure.
- [ ] Implement deterministic path derivation, name/description/body limits, typed YAML serialization and parse round trip. Validate skills before directory creation. Reject missing cwd for Project, existing destination, symlinked `.jcode`/`agents` directories and directory I/O failures.
- [ ] Publish with a same-directory `NamedTempFile`: write full bytes, flush/sync, `persist_noclobber`. Its Drop cleans only the owned staged file on errors. Do not remove or alter existing profiles. Log metadata-only success/failure.
- [ ] Add invalid names, 64-character limit, description/body bounds, missing skills, all modes, punctuation/body round trip, existing malformed file, file/directory symlinks, concurrent creators and failed publish tests. Assert original bytes, no partial file and no leftover staging file.
- [ ] Run the focused creation and existing profile suite, inspect diff and commit owned files with `feat(agents): add safe profile creation`.

## Task 2: Isolated generation and daemon lifecycle

Covers W5, W6 prerequisites, W11. Independent of the writer and TUI.

- [ ] Add recording-provider tests. Capture arguments to `Provider::complete`, return text and verify the resulting body. Assert `messages.len() == 1`, `tools.is_empty()`, `resume_session_id.is_none()`, and absence of transcript/profile/skill sentinel strings. Check input bounds before the provider is called.
- [ ] Add protocol decoding tests using raw JSON so missing request/event variants produce RED through existing deserialization. Run `cargo test -p jcode-protocol --lib agent_instructions` via selfdev.
- [ ] Implement the helper using an independent fork, dedicated instruction-drafting system prompt, one user message, empty tools, absent resume ID and a total 120-second timeout. Collect text only. Reject tool-use events, empty/oversized output and invalid mode. Strip only one complete outer Markdown fence when present. Capture usage with existing provider accounting, without conversation entries. Never route to a fallback model.
- [ ] Add the two request variants, response variant and `Request::id` arms. Implement a connection-owned job in the server helper. One active job per connection, correlated cancellation, no lock held during generation. Cancel/drop on disconnect and session change. Ignore stale generation results. Ensure duplicate requests return an explicit error rather than starting more work.
- [ ] Add stream error, tool-use, empty text, 64-KiB limit, timeout, cancel, duplicate job, disconnect/session-switch and correlation tests. Use bounded durations in a private test seam, not a new user setting. Test the protocol handler with a real socket pair or its existing equivalent.
- [ ] Run base generator, app-core lifecycle and protocol tests. Commit only owned files using `feat(agents): isolate instruction generation`.

## Task 3: Wizard entry, input and manual persistence

Covers W1-W4, W7-W10, W12 and W13 UI prerequisites. Keep App wiring thin and business state in a focused module.

- [ ] Add tests against existing `/agents create` and picker keyboard entry. Before production edits, invoke the command and assert that normal profile activation does not happen and that a creation state starts. Initial RED can assert a visible `Create new agent` entry before introducing wizard types.
- [ ] Add `PickerAction::CreateAgent`, the picker row and exact `/agents create` dispatch ahead of name activation. Preserve `/agents use create`. Initialize an optional wizard state in every App constructor and retain existing SSH/busy guards.
- [ ] Implement an explicit step enum and draft: Location, Name, Description, Purpose, Mode, InstructionChoice, Instructions, Model, Effort, Skills, Review, DiscardConfirmation. Own a saved composer value/cursor so cancellation and save restore the user's draft.
- [ ] Route local and remote key handling and paste into the wizard before ordinary chat dispatch. Use existing composer editing bindings. Enter advances text steps; Escape goes back or offers a non-destructive discard confirmation. All actions remain in memory until confirmed Save.
- [ ] Render using existing inline widget styles. Show title, focused field, validation, Back/Cancel hints and full destination. Wrap/clamp at terminal width and allow scrolling long instructions/review text. On the review screen Save is default and workers-only activation is disabled.
- [ ] Connect model/effort choices to catalog metadata without calling App model-switch setters. Default model Inherit and omitted effort. Resolve optional skills using the working-directory-aware registry. Warn and require acknowledgement for project-over-global shadowing and command collisions. Revalidate on Save.
- [ ] Call `create_profile` only on confirmed Save. On success refresh picker and select the exact new profile action. Save-only preserves model/profile/transcript. Save-and-activate calls the existing selection path and uses its authoritative result. Show file saved independently from activation failure. Keep the draft on save errors.
- [ ] Add keyboard tests for back/edit/discard, no side effects before Save, restored composer, offline/manual success, limits, skills-only, exact path, safe-write failures, existing profile collisions, model/effort draft isolation, all modes and both local/remote key paths.
- [ ] Run `cargo test -p jcode-tui --lib agent_wizard` and existing `agents_`, `inline_interactive`, `agent_model_picker`, `remote_model_switch`, and native SSH guard filters. Commit owned TUI files after GREEN with `feat(tui): guide custom agent creation`.

## Task 4: Async generation integration and recovery

Covers W5-W7 and W10-W11. TUI worker after Task 2 interfaces are available.

- [ ] Add remote socket-pair tests that trigger Generate draft and inspect a GenerateAgentInstructions request. Before its handler exists, assert response text becomes editable draft rather than chat text. Test stale response after cancellation, draft edit and session replacement.
- [ ] Wire local asynchronous completion through a channel and abortable task. Wire daemon-backed completion through the correlated requests. Store generation ID and originating session/draft revision. Reject late responses that do not match all three.
- [ ] Escape while generating aborts or sends CancelAgentInstructions and returns to instruction choice. Disconnect ends pending state without retrying. Auth/network/old-daemon failures retain text and offer Retry/Write myself. Regenerate requires confirmation before replacing edited text.
- [ ] Confirm a response only fills editable body text. It must not change name/path/model/skills or save/activate. Unknown-request protocol errors should give an upgrade/manual-entry message with the draft intact.
- [ ] Run generator lifecycle and TUI tests together. Verify no new conversation message, current-provider mutation or normal processing task. Add a small follow-up commit if required.

## Task 5: Integration review and observed acceptance

- [ ] Review spec compliance first, then code quality/security. Fix all actionable issues in the current branch. Avoid unrelated refactors and pre-existing failures. Check malformed input, stale results, safe writes and no-tool generation trust boundaries.
- [ ] Update `docs/CUSTOM_AGENTS.md` with entry commands, generated/manual steps, destinations, mode/effort behavior, file persistence and activation errors, and the explicit lack of profile ACLs.
- [ ] Run formatting, diff checks and all focused tests through selfdev. Build the TUI binary with `selfdev build target=tui`; if the configured nightly frontend fails, use coordinated `selfdev test` with `export JCODE_PARALLEL_FRONTEND=0; cargo build --profile selfdev -p jcode --bin jcode`.
- [ ] Start an owned isolated daemon/socket and tester using the new binary. Verify manual creation with no provider call, generated draft using the configured provider, edits, Save, Save and activate, cancellation and duplicate-name failure. Use only owned temporary projects/profiles. Capture narrow and normal frames through `client:screen-json` if `tester:frame` is empty.
- [ ] Record W1-W13 individually in a scratch acceptance report: exact test/command, observation, and limitations. Fixture isolation is not evidence about external providers. A real generation failure remains a failed/blocked W6, not a pass based on mocks.
- [ ] Stop owned testers/daemon. Commit changes, build a commit-stamped binary, install/reload with selfdev, confirm running version and repeat core acceptance. Report what is running and its command, not just a successful build. Do not push or merge.

## Plan self-review

W1-W4 map to Tasks 1 and 3. W5-W6 map to Tasks 2, 4 and real acceptance in Task 5. W7-W10 map to Tasks 1, 3 and 4. W11 maps to Tasks 2 and 4. W12-W13 map to Tasks 3 and 5. The generator and writer interfaces above are shared contracts, not placeholders. Any necessary signature change must be communicated to the other owner before editing callers. Tests and live checks listed here are not yet executed.
