//! Per-turn skill suggestion backed by a TypeSafe "System One" decision model
//! (Jev), reached through the shared [`crate::jev::JevClient`] transport.
//!
//! Unlike the embedding path (memory + skills as synthetic memories), this asks
//! a calibrated classifier one `choice` question over every registered skill
//! and injects the winning skill's prompt when confidence clears a threshold.
//!
//! Design mirrors the memory pipeline so it never blocks the provider call:
//! a fresh user turn spawns the decision request in the background, and the
//! *next* fresh user turn consumes whatever the previous request produced.
//! Everything here is opt-in (`agents.skill_suggestion_backend = "jev"`).
//!
//! Provider selection, credential resolution, subscription entitlement checks,
//! request/response size bounds and typed-answer validation all live in the
//! shared Jev client; this module only owns the skill-routing question shape
//! and the per-session pending-suggestion store.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::message::{ContentBlock, Message, Role};

pub const DEFAULT_MIN_CONFIDENCE: f32 = 0.6;
/// Option key meaning "no skill fits"; always offered so the model can abstain.
pub const NONE_OPTION: &str = "none";
/// A pending suggestion older than this is discarded rather than injected.
const PENDING_TTL: Duration = Duration::from_secs(300);
/// Most recent user/assistant turns included in the decision `state`.
const STATE_TURNS: usize = 6;
/// Per-message character cap inside the state, to bound token cost.
const STATE_CHARS_PER_MESSAGE: usize = 1500;
/// Jev caps a choice question at 255 options; keep one slot for `none`.
const MAX_OPTIONS: usize = 254;
/// How long a first-turn inline decision may block the request. Jev answers
/// in ~300ms; anything slower falls back to the background path.
pub const INLINE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Resolved router settings. `None` from [`Self::from_config`] means the
/// feature is off or unusable (no credential route); callers do nothing then.
///
/// Provider, endpoint and credential are owned by the shared Jev client, so
/// only the acceptance threshold is configured here.
#[derive(Debug, Clone)]
pub struct SkillRouterConfig {
    pub min_confidence: f32,
}

impl SkillRouterConfig {
    pub fn from_config() -> Option<Self> {
        let agents = &crate::config::config().agents;
        if !agents.skill_suggestion_backend.eq_ignore_ascii_case("jev") {
            return None;
        }
        // Credential presence only. Subscription entitlement is checked live by
        // the shared client at evaluation time.
        if !crate::jev::JevClient::skill_available() {
            return None;
        }
        Some(Self {
            min_confidence: agents
                .skill_suggestion_min_confidence
                .unwrap_or(DEFAULT_MIN_CONFIDENCE)
                .clamp(0.0, 1.0),
        })
    }
}

/// One candidate the router may pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCandidate {
    pub name: String,
    pub description: String,
}

/// The router's answer for one turn.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillDecision {
    /// Winning option key, or [`NONE_OPTION`].
    pub choice: String,
    pub confidence: f32,
    pub probabilities: Vec<(String, f32)>,
    pub input_tokens: u64,
}

impl SkillDecision {
    /// The skill name to inject, if the model picked a real skill confidently.
    pub fn accepted_skill(&self, min_confidence: f32) -> Option<&str> {
        (self.choice != NONE_OPTION && self.confidence >= min_confidence)
            .then_some(self.choice.as_str())
    }
}

// ── Wire types ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DecisionsResponse {
    answers: HashMap<String, ChoiceAnswer>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize)]
struct ChoiceAnswer {
    choice: String,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    probabilities: HashMap<String, f32>,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
}

const QUESTION_ID: &str = "skill";
const INSTRUCTIONS: &str = "The state is the tail of a conversation between a user and a coding \
agent. Which one of these skills, if activated now, would most help the agent handle the user's \
latest request? Choose `none` when no listed skill is clearly relevant.";

/// Build the single `choice` question this router asks. Pure, so the exact
/// wire shape stays unit-testable. Jev requires 2 to 255 described options,
/// and `none` always occupies one slot so the model can abstain.
pub fn build_questions(candidates: &[SkillCandidate]) -> Map<String, Value> {
    let mut criteria = Map::new();
    for c in candidates.iter().take(MAX_OPTIONS) {
        criteria.insert(c.name.clone(), json!(c.description));
    }
    criteria.insert(
        NONE_OPTION.to_string(),
        json!("No listed skill is clearly useful for this request"),
    );
    let mut questions = Map::new();
    questions.insert(
        QUESTION_ID.to_string(),
        json!({
            "type": "choice",
            "instructions": INSTRUCTIONS,
            "criteria": criteria,
        }),
    );
    questions
}

/// Parse a decisions response body into a [`SkillDecision`].
pub fn parse_response(body: &str) -> Result<SkillDecision> {
    let value: Value = serde_json::from_str(body).context("parse decisions response")?;
    parse_decision(value)
}

/// Convert an already-validated Jev response value into a [`SkillDecision`].
fn parse_decision(value: Value) -> Result<SkillDecision> {
    let parsed: DecisionsResponse =
        serde_json::from_value(value).context("parse decisions response")?;
    let answer = parsed
        .answers
        .get(QUESTION_ID)
        .ok_or_else(|| anyhow::anyhow!("decisions response missing `{QUESTION_ID}` answer"))?;
    let mut probabilities: Vec<(String, f32)> = answer
        .probabilities
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    probabilities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(SkillDecision {
        choice: answer.choice.clone(),
        confidence: answer.confidence,
        probabilities,
        input_tokens: parsed.usage.input_tokens,
    })
}

/// Ask Jev for one decision, via the shared client.
///
/// Callers run this on a dedicated thread (see [`spawn_suggestion`] and
/// [`suggest_inline`]), so a current-thread runtime here never blocks a tokio
/// worker.
pub fn decide(
    _cfg: &SkillRouterConfig,
    state: &str,
    candidates: &[SkillCandidate],
) -> Result<SkillDecision> {
    let questions = build_questions(candidates);
    let client = crate::jev::JevClient::for_skill()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start skill router runtime")?;
    let value = runtime.block_on(client.evaluate(json!(state), questions))?;
    parse_decision(value)
}

// ── State building ───────────────────────────────────────────────────────────

/// Render the last few user/assistant text turns as the decision `state`.
/// Tool calls and results are skipped: the router only needs intent.
pub fn build_state(messages: &[Message]) -> String {
    let mut turns: Vec<String> = Vec::new();
    for msg in messages.iter().rev() {
        let text: String = msg
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let text: String = if text.chars().count() > STATE_CHARS_PER_MESSAGE {
            let mut s: String = text.chars().take(STATE_CHARS_PER_MESSAGE).collect();
            s.push('…');
            s
        } else {
            text.to_string()
        };
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
        };
        turns.push(format!("{role}: {text}"));
        if turns.len() >= STATE_TURNS {
            break;
        }
    }
    turns.reverse();
    turns.join("\n\n")
}

// ── Per-session pending store ────────────────────────────────────────────────

/// A suggestion computed in the background, waiting for the next turn.
#[derive(Debug, Clone)]
pub struct PendingSuggestion {
    pub skill: String,
    pub decision: SkillDecision,
    pub computed_at: Instant,
}

static PENDING: Mutex<Option<HashMap<String, PendingSuggestion>>> = Mutex::new(None);

pub fn set_pending(session_id: &str, suggestion: PendingSuggestion) {
    if let Ok(mut guard) = PENDING.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(session_id.to_string(), suggestion);
    }
}

/// Remove and return the pending suggestion for a session, if still fresh.
pub fn take_pending(session_id: &str) -> Option<PendingSuggestion> {
    let mut guard = PENDING.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    let pending = map.remove(session_id)?;
    (pending.computed_at.elapsed() <= PENDING_TTL).then_some(pending)
}

pub fn clear_pending(session_id: &str) {
    if let Ok(mut guard) = PENDING.lock()
        && let Some(map) = guard.as_mut()
    {
        map.remove(session_id);
    }
}

/// Run one decision and store the result for `session_id`. Returns the
/// accepted suggestion (also stored as pending) so an inline caller can use
/// it directly.
fn run_and_store(
    cfg: &SkillRouterConfig,
    session_id: &str,
    state: &str,
    candidates: &[SkillCandidate],
) -> Option<PendingSuggestion> {
    let started = Instant::now();
    match decide(cfg, state, candidates) {
        Ok(decision) => {
            let top: Vec<String> = decision
                .probabilities
                .iter()
                .take(3)
                .map(|(k, v)| format!("{k}={v:.2}"))
                .collect();
            crate::logging::info(&format!(
                "[skill-router] session={} choice={} confidence={:.2} top=[{}] tokens={} in {}ms",
                session_id,
                decision.choice,
                decision.confidence,
                top.join(", "),
                decision.input_tokens,
                started.elapsed().as_millis()
            ));
            if let Some(skill) = decision.accepted_skill(cfg.min_confidence) {
                let suggestion = PendingSuggestion {
                    skill: skill.to_string(),
                    decision,
                    computed_at: Instant::now(),
                };
                set_pending(session_id, suggestion.clone());
                Some(suggestion)
            } else {
                clear_pending(session_id);
                None
            }
        }
        Err(err) => {
            crate::logging::warn(&format!(
                "[skill-router] session={session_id} request failed: {err:#}"
            ));
            None
        }
    }
}

/// Prepared inputs for one router call, or `None` when nothing should run
/// (feature off, no key, no candidates, empty state).
fn prepare(
    messages: &[Message],
    candidates: Vec<SkillCandidate>,
) -> Option<(SkillRouterConfig, String, Vec<SkillCandidate>)> {
    let cfg = SkillRouterConfig::from_config()?;
    if candidates.is_empty() {
        return None;
    }
    let state = build_state(messages);
    if state.trim().is_empty() {
        return None;
    }
    Some((cfg, state, candidates))
}

/// Fire-and-forget: ask the router in the background and stash the answer
/// for `session_id`. Returns immediately.
pub fn spawn_suggestion(session_id: String, messages: &[Message], candidates: Vec<SkillCandidate>) {
    let Some((cfg, state, candidates)) = prepare(messages, candidates) else {
        return;
    };
    std::thread::Builder::new()
        .name("skill-router".into())
        .spawn(move || {
            run_and_store(&cfg, &session_id, &state, &candidates);
        })
        .ok();
}

/// Ask the router and wait up to [`INLINE_TIMEOUT`] for the answer. Used on
/// the first user turn of a session, where there is no previous turn to have
/// prepared a suggestion. On timeout the request keeps running in the
/// background and its result is stored for the next turn.
pub fn suggest_inline(
    session_id: String,
    messages: &[Message],
    candidates: Vec<SkillCandidate>,
) -> Option<PendingSuggestion> {
    let (cfg, state, candidates) = prepare(messages, candidates)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let sid = session_id.clone();
    std::thread::Builder::new()
        .name("skill-router".into())
        .spawn(move || {
            let result = run_and_store(&cfg, &sid, &state, &candidates);
            let _ = tx.send(result);
        })
        .ok()?;
    match rx.recv_timeout(INLINE_TIMEOUT) {
        Ok(result) => {
            // Consumed directly; do not leave it pending for the next turn.
            clear_pending(&session_id);
            result
        }
        Err(_) => {
            crate::logging::info(&format!(
                "[skill-router] session={session_id} inline decision exceeded {}ms; deferring to next turn",
                INLINE_TIMEOUT.as_millis()
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands() -> Vec<SkillCandidate> {
        vec![
            SkillCandidate {
                name: "pdf".into(),
                description: "Read or create PDFs".into(),
            },
            SkillCandidate {
                name: "gh".into(),
                description: "GitHub CLI".into(),
            },
        ]
    }

    #[test]
    fn question_matches_jev_choice_shape_and_always_offers_none() {
        let questions = build_questions(&cands());
        let q = &questions["skill"];
        assert_eq!(q["type"], "choice");
        assert!(q["instructions"].as_str().unwrap().contains("none"));
        let criteria = q["criteria"].as_object().unwrap();
        assert_eq!(criteria.len(), 3);
        assert_eq!(criteria["gh"], "GitHub CLI");
        assert!(criteria.contains_key("none"));
    }

    /// The shared client rejects malformed questions, so the shape this module
    /// builds must satisfy the same contract the transport enforces.
    #[test]
    fn question_satisfies_shared_client_choice_contract() {
        let questions = build_questions(&cands());
        assert_eq!(questions.len(), 1);
        let q = &questions["skill"];
        assert!(
            q["instructions"].as_str().is_some_and(|s| !s.trim().is_empty()),
            "choice questions require text instructions"
        );
        let criteria = q["criteria"].as_object().unwrap();
        assert!(
            (2..=255).contains(&criteria.len()) && criteria.values().all(Value::is_string),
            "choice questions require 2 to 255 described options"
        );
    }

    #[test]
    fn request_caps_options_below_jev_limit() {
        let many: Vec<SkillCandidate> = (0..400)
            .map(|i| SkillCandidate {
                name: format!("s{i}"),
                description: String::new(),
            })
            .collect();
        let questions = build_questions(&many);
        let n = questions["skill"]["criteria"].as_object().unwrap().len();
        assert_eq!(n, MAX_OPTIONS + 1);
        assert!(n <= 255, "Jev caps choice options at 255");
    }

    #[test]
    fn parses_real_openrouter_response_and_sorts_probabilities() {
        let body = r#"{"model":"typesafe/jev-1.13-20260917","answers":{"skill":{"type":"choice","choice":"jcode_docs","probabilities":{"pdf":0,"find-docs":0.01,"codesearch":0.05,"browser":0.01,"none":0.01,"gh":0,"jcode_docs":0.92},"confidence":0.92}},"usage":{"input_tokens":467,"output_tokens":96,"cost":0.000019614},"id":"gen-dec-1","provider":"TypeSafe"}"#;
        let d = parse_response(body).unwrap();
        assert_eq!(d.choice, "jcode_docs");
        assert!((d.confidence - 0.92).abs() < 1e-6);
        assert_eq!(d.input_tokens, 467);
        assert_eq!(d.probabilities[0].0, "jcode_docs");
        assert_eq!(d.probabilities[1].0, "codesearch");
        assert_eq!(d.accepted_skill(0.6), Some("jcode_docs"));
        assert_eq!(d.accepted_skill(0.95), None);
    }

    #[test]
    fn none_choice_is_never_accepted() {
        let d = SkillDecision {
            choice: NONE_OPTION.into(),
            confidence: 1.0,
            probabilities: vec![],
            input_tokens: 0,
        };
        assert_eq!(d.accepted_skill(0.0), None);
    }

    #[test]
    fn state_uses_recent_text_turns_only_in_order() {
        let mut msgs = Vec::new();
        for i in 0..10 {
            msgs.push(Message::user(&format!("u{i}")));
            msgs.push(Message::assistant_text(&format!("a{i}")));
        }
        let state = build_state(&msgs);
        let lines: Vec<&str> = state.split("\n\n").collect();
        assert_eq!(lines.len(), STATE_TURNS);
        assert_eq!(lines.first(), Some(&"User: u7"));
        assert_eq!(lines.last(), Some(&"Assistant: a9"));
    }

    #[test]
    fn state_truncates_long_messages() {
        let long = "x".repeat(STATE_CHARS_PER_MESSAGE + 50);
        let state = build_state(&[Message::user(&long)]);
        assert!(state.ends_with('…'));
        assert!(state.chars().count() < STATE_CHARS_PER_MESSAGE + 20);
    }

    #[test]
    fn pending_store_round_trips_and_clears() {
        let sid = "session_test_skill_router";
        clear_pending(sid);
        assert!(take_pending(sid).is_none());
        set_pending(
            sid,
            PendingSuggestion {
                skill: "pdf".into(),
                decision: SkillDecision {
                    choice: "pdf".into(),
                    confidence: 0.9,
                    probabilities: vec![],
                    input_tokens: 1,
                },
                computed_at: Instant::now(),
            },
        );
        assert_eq!(take_pending(sid).map(|p| p.skill), Some("pdf".into()));
        assert!(take_pending(sid).is_none(), "take consumes");
    }

    #[test]
    fn stale_pending_is_dropped() {
        let sid = "session_test_skill_router_stale";
        set_pending(
            sid,
            PendingSuggestion {
                skill: "pdf".into(),
                decision: SkillDecision {
                    choice: "pdf".into(),
                    confidence: 0.9,
                    probabilities: vec![],
                    input_tokens: 1,
                },
                computed_at: Instant::now() - PENDING_TTL - Duration::from_secs(1),
            },
        );
        assert!(take_pending(sid).is_none());
    }
}
