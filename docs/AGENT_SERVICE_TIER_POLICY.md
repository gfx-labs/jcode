# Independent main and worker OpenAI service tiers

Main sessions and spawned workers can have different default OpenAI speed/billing policies:

```toml
[provider]
openai_service_tier = "off"

[agents]
swarm_model = "openai-oauth:gpt-5.6-luna"
swarm_openai_service_tier = "priority"
```

This example leaves main OpenAI sessions at standard service and requests priority (Fast) for OpenAI swarm workers. Priority availability and increased usage/billing depend on your OpenAI account and model. This is a request for a service tier, not a separate model ID or a guarantee of latency.

## Values and scope

- `priority` (alias `fast`), `flex`, or `off` (alias `standard`).
- Unset, empty, or `inherit` preserves the worker provider's existing default behavior.
- `JCODE_SWARM_OPENAI_SERVICE_TIER` overrides the worker setting. `JCODE_OPENAI_SERVICE_TIER` continues to override the main provider setting.
- The worker policy applies after model selection for inline/headless workers, visible worker windows, and short swarm tasks. It only changes OpenAI providers. Other providers, ordinary user splits/transfers, and memory sidecars are not assigned this worker policy.
- New workers snapshot the policy in their saved session so visible attachment and later resume do not accidentally use the main default. Changing the config affects newly spawned workers, not existing worker sessions.
- `/fast on` and `/fast off` remain local to the active session. They do not change the other agent's provider. These temporary overrides last until a model/route switch or resume, which restores the saved worker spawn policy. `/fast default ...` updates the main OpenAI default, not the worker setting.

Independent model defaults remain available through `provider.default_model` and `agents.swarm_model`. Worker tier configuration does not create a new model-picker entry.
