use super::*;
use std::collections::BTreeMap;

fn route(model: &str, provider: &str, api_method: &str, available: bool) -> ModelRoute {
    ModelRoute {
        model: model.to_string(),
        provider: provider.to_string(),
        api_method: api_method.to_string(),
        available,
        detail: String::new(),
        cheapness: None,
        usage: None,
    }
}

fn routes() -> Vec<ModelRoute> {
    vec![
        route("gpt-6.1-sol", "OpenAI", "openai-oauth", true),
        route("gpt-6.1-sol", "OpenAI", "openrouter", true),
        route("claude-opus-5-5", "Anthropic", "claude-oauth", true),
        route("gpt-6-luna", "OpenAI", "openai-oauth", true),
        route("glm-5.3", "Z.AI", "openai-compatible:zai", true),
        route("gpt-6-astra", "OpenAI", "openai-oauth", false),
    ]
}

fn config() -> SwarmRouterConfig {
    SwarmRouterConfig {
        enabled: true,
        candidates: vec![
            "openai-oauth:gpt-6.1-sol".into(),
            "claude-oauth:claude-opus-5-5".into(),
            "openai-oauth:gpt-6-luna".into(),
            "glm-5.3".into(),
            "openai-oauth:gpt-6-astra".into(),
        ],
        descriptions: BTreeMap::from([
            ("openai-oauth:gpt-6.1-sol".into(), "Hard work".into()),
            ("glm-5.3".into(), "Cheap bulk work".into()),
        ]),
        efforts: BTreeMap::from([
            ("openai-oauth:gpt-6.1-sol".into(), "high".into()),
            ("claude-oauth:claude-opus-5-5".into(), "medium".into()),
        ]),
        ..SwarmRouterConfig::default()
    }
}

#[test]
fn candidates_resolve_to_available_route_specs_in_config_order() {
    let candidates = route_candidates(&config(), &routes());
    let specs: Vec<&str> = candidates.iter().map(|c| c.spec.as_str()).collect();
    assert_eq!(
        specs,
        [
            "openai-oauth:gpt-6.1-sol",
            "claude-oauth:claude-opus-5-5",
            "openai-oauth:gpt-6-luna",
            "zai:glm-5.3",
        ]
    );
    assert_eq!(candidates[0].description, "Hard work");
    assert_eq!(candidates[0].effort.as_deref(), Some("high"));
    assert_eq!(candidates[1].effort.as_deref(), Some("medium"));
    // Bare ids pick up descriptions keyed as written in config.
    assert_eq!(candidates[3].description, "Cheap bulk work");
    // Undescribed candidates fall back to catalog text.
    assert!(candidates[2].description.contains("gpt-6-luna"));
}

#[test]
fn empty_candidates_offer_every_available_route() {
    let cfg = SwarmRouterConfig {
        enabled: true,
        ..SwarmRouterConfig::default()
    };
    let candidates = route_candidates(&cfg, &routes());
    assert_eq!(candidates.len(), 5);
    assert!(!candidates.iter().any(|c| c.spec.contains("astra")));
}

#[test]
fn questions_offer_one_choice_per_candidate() {
    let candidates = route_candidates(&config(), &routes());
    let questions = build_questions(&candidates);
    let question = &questions[QUESTION_ID];
    assert_eq!(question["type"], "choice");
    let criteria = question["criteria"].as_object().unwrap();
    assert_eq!(criteria.len(), 4);
    assert_eq!(criteria["zai:glm-5.3"], "Cheap bulk work");
}

#[test]
fn parse_choice_accepts_only_offered_candidates() {
    let candidates = route_candidates(&config(), &routes());
    let answer = |choice: &str| json!({"answers": {"model": {"type": "choice", "choice": choice, "confidence": 0.8}}});
    let routed = parse_choice(answer("claude-oauth:claude-opus-5-5"), &candidates).unwrap();
    assert_eq!(routed.spec, "claude-oauth:claude-opus-5-5");
    assert_eq!(routed.effort.as_deref(), Some("medium"));
    assert!(parse_choice(answer("openai-oauth:gpt-6-astra"), &candidates).is_none());
    assert!(parse_choice(json!({"answers": {}}), &candidates).is_none());
}

#[test]
fn explicit_model_bypasses_routing() {
    assert!(should_route(None));
    assert!(should_route(Some("  ")));
    assert!(!should_route(Some("inherit")));
    assert!(!should_route(Some("gpt-6-luna")));
}

#[tokio::test]
async fn disabled_or_taskless_router_selects_nothing() {
    let mut cfg = config();
    assert!(
        select_swarm_model(&cfg, &RoutingContext::default(), &routes())
            .await
            .is_none()
    );
    cfg.enabled = false;
    let context = RoutingContext::from_task(Some("fix the bug"));
    assert!(
        select_swarm_model(&cfg, &context, &routes())
            .await
            .is_none()
    );
}

#[test]
fn state_includes_plan_metadata_and_omits_empty_fields() {
    let context = RoutingContext {
        task: Some("  Verify the parser fix  ".into()),
        label: Some(" ".into()),
        kind: Some("verify".into()),
        subsystem: Some("parser".into()),
        file_scope: vec!["src/parse.rs".into(), " ".into()],
        plan_mode: Some("deep".into()),
    };
    let state = context.to_state().unwrap();
    assert_eq!(state["task"], "Verify the parser fix");
    assert_eq!(state["node_kind"], "verify");
    assert_eq!(state["subsystem"], "parser");
    assert_eq!(state["swarm_mode"], "deep");
    assert_eq!(state["file_scope"], json!(["src/parse.rs"]));
    assert!(state.get("label").is_none());
}

#[test]
fn state_requires_task_text_and_bounds_it() {
    assert!(RoutingContext::from_task(Some("   ")).to_state().is_none());
    let long = "x".repeat(MAX_TASK_CHARS + 50);
    let state = RoutingContext::from_task(Some(&long)).to_state().unwrap();
    assert_eq!(
        state["task"].as_str().unwrap().chars().count(),
        MAX_TASK_CHARS
    );
}

#[test]
fn plan_metadata_state_passes_shared_jev_validation_shape() {
    let context = RoutingContext {
        task: Some("Implement it".into()),
        kind: Some("implement".into()),
        ..RoutingContext::default()
    };
    // Jev accepts object state, and the question keeps a valid choice shape.
    assert!(context.to_state().unwrap().is_object());
    let questions = build_questions(&route_candidates(&config(), &routes()));
    assert!(
        (2..=255).contains(
            &questions[QUESTION_ID]["criteria"]
                .as_object()
                .unwrap()
                .len()
        )
    );
}

#[tokio::test]
async fn single_available_candidate_needs_no_decision() {
    let cfg = SwarmRouterConfig {
        enabled: true,
        candidates: vec!["glm-5.3".into()],
        efforts: BTreeMap::from([("glm-5.3".into(), "low".into())]),
        ..SwarmRouterConfig::default()
    };
    let context = RoutingContext::from_task(Some("summarize"));
    let routed = select_swarm_model(&cfg, &context, &routes()).await.unwrap();
    assert_eq!(routed.spec, "zai:glm-5.3");
    assert_eq!(routed.effort.as_deref(), Some("low"));
}
