#[test]
fn agent_profile_fork_preserves_effort_and_isolates_provider_model_state() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            for (active, child, prefix) in [
                (
                    ActiveProvider::Copilot,
                    StubExternalRuntime::copilot(),
                    "copilot",
                ),
                (
                    ActiveProvider::Antigravity,
                    StubExternalRuntime::antigravity(),
                    "antigravity",
                ),
                (
                    ActiveProvider::Gemini,
                    StubExternalRuntime::new("gemini", "Gemini", "https", gemini::AVAILABLE_MODELS),
                    "gemini",
                ),
                (
                    ActiveProvider::OpenAI,
                    StubExternalRuntime::openai(),
                    "openai",
                ),
                (
                    ActiveProvider::Claude,
                    StubExternalRuntime::anthropic(),
                    "anthropic",
                ),
            ] {
                let original_model = child.models[0];
                let next_model = child.models[1];
                child.set_reasoning_effort("high").unwrap();
                let child: Arc<dyn Provider> = Arc::new(child);
                let provider = test_multi_provider_with_cursor();
                match active {
                    ActiveProvider::Copilot => *provider.copilot_api.write().unwrap() = Some(child),
                    ActiveProvider::Antigravity => {
                        *provider.antigravity.write().unwrap() = Some(child)
                    }
                    ActiveProvider::Gemini => *provider.gemini.write().unwrap() = Some(child),
                    ActiveProvider::OpenAI => *provider.openai.write().unwrap() = Some(child),
                    ActiveProvider::Claude => *provider.anthropic.write().unwrap() = Some(child),
                    _ => unreachable!(),
                }
                *provider.active.write().unwrap() = active;
                let original_effort = provider.reasoning_effort();
                let fork = provider.fork();
                assert_eq!(fork.reasoning_effort(), original_effort, "{prefix}");
                fork.set_model(&format!("{prefix}:{next_model}")).unwrap();
                if original_effort.is_some() {
                    fork.set_reasoning_effort("low").unwrap();
                }
                assert_eq!(provider.model(), original_model, "{prefix}");
                assert_eq!(provider.reasoning_effort(), original_effort, "{prefix}");
            }
        });
    });
}
