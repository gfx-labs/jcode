//! MCP management tool - connect, disconnect, list, reload MCP servers

use crate::mcp::{ContentBlock, McpManager, McpServerConfig, dispatch_name};
use crate::tool::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Deserialize)]
struct McpSearchInput {
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Debug, Serialize)]
struct McpSearchResult {
    name: String,
    server: String,
    tool: String,
    description: String,
    input_schema: Value,
}

/// Fixed MCP discovery surface used when individual server definitions are deferred.
pub struct McpSearchTool {
    manager: Arc<RwLock<McpManager>>,
    registry: Option<super::WeakRegistry>,
}

impl McpSearchTool {
    pub fn new(manager: Arc<RwLock<McpManager>>) -> Self {
        Self {
            manager,
            registry: None,
        }
    }

    pub fn with_registry(mut self, registry: crate::tool::Registry) -> Self {
        self.registry = Some(registry.downgrade());
        self
    }
}

#[async_trait]
impl Tool for McpSearchTool {
    fn name(&self) -> &str {
        "mcp_search"
    }

    fn description(&self) -> &str {
        "Search available MCP tools by server, name, or description. Returns callable names and input schemas."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Optional exact MCP server name."
                },
                "query": {
                    "type": "string",
                    "description": "Optional case-insensitive name or description search."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: McpSearchInput = serde_json::from_value(input)?;
        let server_filter = params
            .server
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let query = params
            .query
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase);
        let manager = self.manager.read().await;
        let catalog = manager.searchable_tools().await;
        drop(manager);

        let names = crate::mcp::dispatch_names(&catalog);
        let matches: Vec<McpSearchResult> = catalog
            .into_iter()
            .zip(names)
            .filter_map(|((server, tool), name)| {
                if server_filter.is_some_and(|wanted| wanted != server) {
                    return None;
                }
                let legacy_name = dispatch_name(&server, &tool.name);
                let allowed = self
                    .registry
                    .as_ref()
                    .and_then(|r| r.upgrade())
                    .map_or_else(
                        || {
                            super::session_mcp_alias_is_allowed(
                                &ctx.session_id,
                                &name,
                                &legacy_name,
                                "mcp_search",
                            )
                        },
                        |r| {
                            r.mcp_dispatch_is_allowed(
                                &ctx.session_id,
                                &server,
                                &tool.name,
                                &name,
                                "mcp_search",
                            )
                        },
                    );
                if !allowed {
                    return None;
                }
                if let Some(query) = &query {
                    let description = tool.description.as_deref().unwrap_or_default();
                    if !name.to_ascii_lowercase().contains(query)
                        && !server.to_ascii_lowercase().contains(query)
                        && !tool.name.to_ascii_lowercase().contains(query)
                        && !description.to_ascii_lowercase().contains(query)
                    {
                        return None;
                    }
                }
                Some(McpSearchResult {
                    name,
                    server,
                    tool: tool.name,
                    description: tool.description.unwrap_or_else(|| "MCP tool".to_string()),
                    input_schema: tool.input_schema,
                })
            })
            .collect();

        Ok(ToolOutput::new(serde_json::to_string_pretty(&matches)?)
            .with_title(format!("MCP tools ({})", matches.len())))
    }
}

#[derive(Debug, Deserialize)]
struct McpCallInput {
    server: String,
    tool: String,
    #[serde(default)]
    arguments: Value,
}

/// Fixed MCP execution surface used when individual server definitions are deferred.
pub struct McpCallTool {
    manager: Arc<RwLock<McpManager>>,
    registry: Option<super::WeakRegistry>,
}

impl McpCallTool {
    pub fn new(manager: Arc<RwLock<McpManager>>) -> Self {
        Self {
            manager,
            registry: None,
        }
    }

    pub fn with_registry(mut self, registry: crate::tool::Registry) -> Self {
        self.registry = Some(registry.downgrade());
        self
    }
}

#[async_trait]
impl Tool for McpCallTool {
    fn name(&self) -> &str {
        "mcp_call"
    }

    fn description(&self) -> &str {
        "Call an MCP server tool discovered with mcp_search."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {"type": "string", "description": "MCP server name."},
                "tool": {"type": "string", "description": "Raw MCP tool name."},
                "arguments": {
                    "type": "object",
                    "additionalProperties": true,
                    "description": "Arguments matching the input schema returned by mcp_search."
                }
            },
            "required": ["server", "tool", "arguments"]
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let mut params: McpCallInput = serde_json::from_value(input)?;
        let dispatched_name = dispatch_name(&params.server, &params.tool);
        // Check the current alias too: a per-alias deny must not be bypassed
        // by spelling the original server/tool pair through mcp_call.
        let catalog = self.manager.read().await.searchable_tools().await;
        let names = crate::mcp::dispatch_names(&catalog);
        let alias = catalog
            .iter()
            .zip(&names)
            .find(|((server, tool), _)| server == &params.server && tool.name == params.tool)
            .map(|(_, alias)| alias.as_str())
            .unwrap_or(&dispatched_name);
        let allowed = self
            .registry
            .as_ref()
            .and_then(|r| r.upgrade())
            .map_or_else(
                || {
                    super::session_mcp_alias_is_allowed(
                        &ctx.session_id,
                        alias,
                        &dispatched_name,
                        "mcp_call",
                    )
                },
                |r| {
                    r.mcp_dispatch_is_allowed(
                        &ctx.session_id,
                        &params.server,
                        &params.tool,
                        alias,
                        "mcp_call",
                    )
                },
            );
        if !allowed {
            anyhow::bail!("MCP tool '{}' is not allowed", alias);
        }
        if params.arguments.is_null() {
            params.arguments = Value::Object(serde_json::Map::new());
        }

        let manager = self.manager.read().await;
        let result = manager
            .call_tool(&params.server, &params.tool, params.arguments)
            .await?;
        drop(manager);

        let mut output_parts = Vec::new();
        for block in result.content {
            match block {
                ContentBlock::Text { text } => output_parts.push(text),
                ContentBlock::Image { data, mime_type } => {
                    output_parts.push(format!("[Image: {} ({} bytes)]", mime_type, data.len()));
                }
                ContentBlock::Resource { resource } => {
                    if let Some(text) = resource.text {
                        output_parts.push(text);
                    } else if let Some(blob) = resource.blob {
                        output_parts.push(format!(
                            "[Resource: {} ({} bytes)]",
                            resource.uri,
                            blob.len()
                        ));
                    } else {
                        output_parts.push(format!("[Resource: {}]", resource.uri));
                    }
                }
            }
        }
        let output = output_parts.join("\n");
        let title = format!("mcp:{}:{}", params.server, params.tool);
        if result.is_error {
            Ok(ToolOutput::new(format!("Error: {}", output)).with_title(title))
        } else {
            Ok(ToolOutput::new(output).with_title(title))
        }
    }
}

#[derive(Debug, Deserialize)]
struct McpToolInput {
    action: String,
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    replace: Option<bool>,
}

fn parse_scope(scope: Option<&str>) -> Result<crate::mcp::McpConfigScope> {
    match scope.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("global") | Some("user") => Ok(crate::mcp::McpConfigScope::Global),
        Some("project") | Some("local") => Ok(crate::mcp::McpConfigScope::Project),
        Some(other) => anyhow::bail!("unknown scope '{other}' (use global or project)"),
    }
}

pub struct McpManagementTool {
    manager: Arc<RwLock<McpManager>>,
    registry: Option<crate::tool::WeakRegistry>,
}

impl McpManagementTool {
    pub fn new(manager: Arc<RwLock<McpManager>>) -> Self {
        Self {
            manager,
            registry: None,
        }
    }

    pub fn with_registry(mut self, registry: crate::tool::Registry) -> Self {
        self.registry = Some(registry.downgrade());
        self
    }
}

#[async_trait]
impl Tool for McpManagementTool {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "Manage MCP (Model Context Protocol) servers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "action": {
                    "type": "string",
                    "enum": ["list", "connect", "disconnect", "reload", "enable", "disable", "add", "auth", "logout"],
                    "description": "Action."
                },
                "server": {
                    "type": "string",
                    "description": "Server name."
                },
                "command": {
                    "type": "string",
                    "description": "Server command."
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Command args."
                },
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Server env."
                },
                "url": {
                    "type": "string",
                    "description": "Remote server URL for add."
                },
                "replace": {
                    "type": "boolean",
                    "description": "For add: overwrite an existing entry with the same name."
                },
                "scope": {
                    "type": "string",
                    "enum": ["global", "project"],
                    "description": "Config scope for enable/disable/add. Default global."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: McpToolInput = serde_json::from_value(input)?;
        let started = std::time::Instant::now();
        let action = params.action.clone();
        let server = params.server.clone().unwrap_or_else(|| "none".to_string());
        crate::logging::event_info(
            "MCP_LIFECYCLE",
            vec![
                ("phase", "management_start".to_string()),
                ("action", action.clone()),
                ("server", server.clone()),
                ("session_id", ctx.session_id.clone()),
                ("tool_call_id", ctx.tool_call_id.clone()),
            ],
        );

        let result = match params.action.as_str() {
            "list" | "status" => self.list_servers(ctx.working_dir.clone()).await,
            "connect" => self.connect_server(params, &ctx.session_id).await,
            "disconnect" => self.disconnect_server(params).await,
            "reload" => self.reload_config(&ctx.session_id).await,
            "enable" | "disable" => {
                let enabled = params.action == "enable";
                self.set_enabled(params, enabled, &ctx).await
            }
            "add" => self.add_remote(params, &ctx).await,
            "auth" => self.authenticate(params, &ctx).await,
            "logout" => self.logout(params, &ctx).await,
            _ => Ok(ToolOutput::new(format!(
                "Unknown action: {}. Use 'list', 'connect', 'disconnect', 'reload', 'enable', 'disable', 'add', 'auth', or 'logout'.",
                params.action
            ))),
        };

        match &result {
            Ok(_) => crate::logging::event_info(
                "MCP_LIFECYCLE",
                vec![
                    ("phase", "management_done".to_string()),
                    ("action", action),
                    ("server", server),
                    ("session_id", ctx.session_id),
                    ("tool_call_id", ctx.tool_call_id),
                    ("status", "ok".to_string()),
                    ("elapsed_ms", started.elapsed().as_millis().to_string()),
                ],
            ),
            Err(error) => crate::logging::event_warn(
                "MCP_LIFECYCLE",
                vec![
                    ("phase", "management_done".to_string()),
                    ("action", action),
                    ("server", server),
                    ("session_id", ctx.session_id),
                    ("tool_call_id", ctx.tool_call_id),
                    ("status", "error".to_string()),
                    ("error", error.to_string()),
                    ("elapsed_ms", started.elapsed().as_millis().to_string()),
                ],
            ),
        }

        result
    }
}

// Helper for tests to update cached server names
impl McpManagementTool {
    pub fn manager(&self) -> &Arc<RwLock<McpManager>> {
        &self.manager
    }
}

impl McpManagementTool {
    async fn list_servers(&self, project_dir: Option<std::path::PathBuf>) -> Result<ToolOutput> {
        let manager = self.manager.read().await;
        let servers = manager.connected_servers().await;
        let all_tools = manager.all_tools().await;
        // Configured-but-not-connected servers, including disabled ones
        // (issue #436), so the full config state is visible.
        let mut configured: Vec<(String, bool)> = manager
            .config()
            .servers
            .iter()
            .filter(|(name, _)| !servers.contains(name))
            .map(|(name, cfg)| (name.clone(), cfg.is_enabled()))
            .collect();
        configured.sort();

        if servers.is_empty() && configured.is_empty() {
            return Ok(ToolOutput::new(
                "No MCP servers connected.\n\n\
                Add a remote server with /mcp add <name> <url> [--project],\n\
                for example: /mcp add figma https://mcp.figma.com/mcp\n\n\
                Or add servers to ~/.jcode/mcp.json or .jcode/mcp.json and use {\"action\": \"reload\"}.\n\
                .claude/mcp.json is also supported for compatibility."
            ).with_title("MCP: No servers"));
        }

        let mut output = String::new();
        output.push_str(&format!("Connected MCP servers: {}\n\n", servers.len()));

        let names = crate::mcp::dispatch_names(&all_tools);
        for server in &servers {
            output.push_str(&format!("## {}\n", server));
            let server_tools: Vec<_> = all_tools
                .iter()
                .zip(&names)
                .filter(|((owner, _), _)| owner == server)
                .collect();

            if server_tools.is_empty() {
                output.push_str("  (no tools)\n");
            } else {
                for ((_, tool), fallback) in server_tools {
                    let name = self
                        .registry
                        .as_ref()
                        .and_then(|r| r.upgrade())
                        .and_then(|r| r.mcp_alias(server, &tool.name))
                        .unwrap_or_else(|| fallback.clone());
                    output.push_str(&format!(
                        "  - {}: {}\n",
                        name,
                        tool.description.as_deref().unwrap_or("(no description)")
                    ));
                }
            }
            output.push('\n');
        }

        drop(manager);
        output.push_str(&self.status_table(project_dir.as_deref(), &servers).await);
        Ok(ToolOutput::new(output).with_title("MCP: Server list"))
    }

    async fn connect_server(&self, params: McpToolInput, session_id: &str) -> Result<ToolOutput> {
        let server_name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for connect action"))?;

        // With an explicit command this is an ad-hoc connect. Without one, fall
        // back to the configured server of that name, which also lets disabled
        // configured servers be connected on demand, session-scoped, without
        // rewriting config (issue #436).
        let config = if let Some(command) = params.command {
            McpServerConfig {
                command,
                args: params.args.unwrap_or_default(),
                env: params.env.unwrap_or_default(),
                shared: true,
                transport: None,
                url: None,
                headers: std::collections::HashMap::new(),
                enabled: None,
                disabled: None,
                timeout_secs: None,
                oauth: None,
            }
        } else {
            let manager = self.manager.read().await;
            let configured = manager.config().servers.get(&server_name).cloned();
            drop(manager);
            configured.ok_or_else(|| {
                anyhow::anyhow!(
                    "'command' is required for connect action ('{}' is not in the MCP config)",
                    server_name
                )
            })?
        };

        let manager = self.manager.read().await;

        // Check if already connected
        let connected = manager.connected_servers().await;
        if connected.contains(&server_name) {
            return Ok(ToolOutput::new(format!(
                "Server '{}' is already connected. Use 'disconnect' first to reconnect.",
                server_name
            ))
            .with_title("MCP: Already connected"));
        }
        drop(manager);

        // Connect
        let manager = self.manager.read().await;
        match manager.connect(&server_name, &config).await {
            Ok(()) => {
                let tools = manager.all_tools().await;
                let connected = manager.connected_servers().await;
                drop(manager);
                let registry = self.registry.as_ref().and_then(|r| r.upgrade());
                if let Some(registry) = &registry {
                    registry
                        .refresh_mcp_tools(
                            crate::mcp::create_mcp_tools_from_cached_many(
                                &tools,
                                Arc::clone(&self.manager),
                            ),
                            &connected,
                        )
                        .await;
                }
                let names = crate::mcp::dispatch_names(&tools);
                let server_tools: Vec<_> = tools
                    .iter()
                    .zip(&names)
                    .filter(|((server, _), _)| server == &server_name)
                    .collect();
                let mut output = format!(
                    "Connected to MCP server '{}'\n\nAvailable tools ({}):\n",
                    server_name,
                    server_tools.len()
                );
                for ((_, tool), fallback) in server_tools {
                    let name = registry
                        .as_ref()
                        .and_then(|r| r.mcp_alias(&server_name, &tool.name))
                        .unwrap_or_else(|| fallback.clone());
                    output.push_str(&format!(
                        "  - {}: {}\n",
                        name,
                        tool.description.as_deref().unwrap_or("(no description)")
                    ));
                }

                Ok(ToolOutput::new(output).with_title(format!("MCP: Connected {}", server_name)))
            }
            Err(e) => {
                crate::logging::event_warn(
                    "MCP_LIFECYCLE",
                    vec![
                        ("phase", "connect_failed".to_string()),
                        ("server", server_name.clone()),
                        ("session_id", session_id.to_string()),
                        ("error", e.to_string()),
                    ],
                );
                Ok(
                    ToolOutput::new(format!("Failed to connect to '{}': {}", server_name, e))
                        .with_title("MCP: Connection failed"),
                )
            }
        }
    }

    async fn disconnect_server(&self, params: McpToolInput) -> Result<ToolOutput> {
        let server_name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for disconnect action"))?;

        let manager = self.manager.read().await;
        let connected = manager.connected_servers().await;

        if !connected.contains(&server_name) {
            return Ok(ToolOutput::new(format!(
                "Server '{}' is not connected.\n\nConnected servers: {}",
                server_name,
                if connected.is_empty() {
                    "(none)".to_string()
                } else {
                    connected.join(", ")
                }
            ))
            .with_title("MCP: Not connected"));
        }
        drop(manager);

        let manager = self.manager.read().await;
        manager.disconnect(&server_name).await?;
        drop(manager);

        // Unregister tools for this server
        if let Some(registry) = self
            .registry
            .as_ref()
            .and_then(|registry| registry.upgrade())
        {
            let removed = registry.unregister_mcp_server(&server_name).await;
            let connected = self.manager.read().await.connected_servers().await;
            registry
                .refresh_mcp_tools(
                    crate::mcp::create_mcp_tools(Arc::clone(&self.manager)).await,
                    &connected,
                )
                .await;
            crate::logging::event_info(
                "MCP_LIFECYCLE",
                vec![
                    ("phase", "tools_unregistered".to_string()),
                    ("server", server_name.clone()),
                    ("removed_tool_count", removed.len().to_string()),
                ],
            );
        }

        Ok(
            ToolOutput::new(format!("Disconnected from MCP server '{}'", server_name))
                .with_title(format!("MCP: Disconnected {}", server_name)),
        )
    }

    async fn reload_config(&self, session_id: &str) -> Result<ToolOutput> {
        // Load fresh config, resolved against the session's project directory
        // rather than the server process cwd (issue #420).
        let config = self.manager.read().await.load_fresh_config();

        if config.servers.is_empty() {
            // Unregister all existing MCP tools before reporting empty
            if let Some(registry) = self
                .registry
                .as_ref()
                .and_then(|registry| registry.upgrade())
            {
                registry.unregister_prefix("mcp__").await;
            }
            return Ok(ToolOutput::new(
                "No servers found in config.\n\n\
                Add servers to ~/.jcode/mcp.json (global) or .jcode/mcp.json (project):\n\
                {\n  \"servers\": {\n    \"server-name\": {\n      \"command\": \"/path/to/server\",\n      \"args\": [],\n      \"env\": {},\n      \"shared\": true\n    }\n  }\n}\n\n\
                .claude/mcp.json is also supported for compatibility."
            ).with_title("MCP: Empty config"));
        }

        // Unregister all existing MCP server tools before reload
        if let Some(registry) = self
            .registry
            .as_ref()
            .and_then(|registry| registry.upgrade())
        {
            registry.unregister_prefix("mcp__").await;
        }

        let mut manager = self.manager.write().await;
        let (successes, failures) = manager.reload().await?;

        let servers = manager.connected_servers().await;
        let all_tools = manager.all_tools().await;
        drop(manager);

        // Re-register tools from fresh connections
        if let Some(registry) = self
            .registry
            .as_ref()
            .and_then(|registry| registry.upgrade())
        {
            let mcp_tools = crate::mcp::create_mcp_tools(Arc::clone(&self.manager)).await;
            registry.reconcile_mcp_tools(mcp_tools).await;
        }

        let enabled_count = config
            .servers
            .values()
            .filter(|cfg| cfg.is_enabled())
            .count();
        let disabled_count = config.servers.len() - enabled_count;
        let mut output = format!(
            "Reloaded MCP config. Connected: {}/{}\n\n",
            successes, enabled_count
        );
        if disabled_count > 0 {
            output.push_str(&format!(
                "{} server(s) disabled in config (kept, not spawned).\n\n",
                disabled_count
            ));
        }

        // Show failures first
        if !failures.is_empty() {
            crate::logging::event_warn(
                "MCP_LIFECYCLE",
                vec![
                    ("phase", "reload_connect_failures".to_string()),
                    ("session_id", session_id.to_string()),
                    ("failure_count", failures.len().to_string()),
                    (
                        "servers",
                        failures
                            .iter()
                            .map(|(name, _)| name.clone())
                            .collect::<Vec<_>>()
                            .join(","),
                    ),
                ],
            );
            output.push_str("## Connection Failures\n");
            for (name, error) in &failures {
                output.push_str(&format!("  - {}: {}\n", name, error));
            }
            output.push('\n');
        }

        let names = crate::mcp::dispatch_names(&all_tools);
        for server in &servers {
            output.push_str(&format!("## {}\n", server));
            let server_tools: Vec<_> = all_tools
                .iter()
                .zip(&names)
                .filter(|((owner, _), _)| owner == server)
                .collect();

            for (_, name) in server_tools {
                output.push_str(&format!("  - {}\n", name));
            }
            output.push('\n');
        }

        Ok(ToolOutput::new(output).with_title("MCP: Reloaded"))
    }
}

impl McpManagementTool {
    fn project_dir(ctx: &ToolContext) -> Option<std::path::PathBuf> {
        ctx.working_dir.clone()
    }

    /// Fresh config entry for `name`, including disabled servers.
    fn configured_server(&self, name: &str, ctx: &ToolContext) -> Option<McpServerConfig> {
        crate::mcp::McpConfig::load_for_dir(Self::project_dir(ctx).as_deref())
            .servers
            .get(name)
            .cloned()
    }

    /// Table of every server in the manager's effective config with scope,
    /// state, and auth status. Scope is looked up on disk only for names the
    /// manager already knows, so foreign definitions never leak in.
    async fn status_table(
        &self,
        project_dir: Option<&std::path::Path>,
        connected: &[String],
    ) -> String {
        let manager = self.manager.read().await;
        let mut names: Vec<(String, McpServerConfig)> = manager
            .config()
            .servers
            .iter()
            .map(|(n, c)| (n.clone(), c.clone()))
            .collect();
        drop(manager);
        if names.is_empty() {
            return String::new();
        }
        names.sort_by(|a, b| a.0.cmp(&b.0));
        let scopes: HashMap<String, &'static str> =
            crate::mcp::McpConfig::list_configured(project_dir)
                .into_iter()
                .map(|s| (s.name, s.scope.as_str()))
                .collect();
        let mut out = String::from("## Configured servers\n");
        for (name, cfg) in &names {
            let state = if connected.contains(name) {
                "connected"
            } else if cfg.is_enabled() {
                "enabled, not connected"
            } else {
                "disabled in config"
            };
            let auth = match crate::mcp::oauth::auth_status(cfg) {
                crate::mcp::oauth::McpAuthStatus::NotApplicable => String::new(),
                s => format!(", auth: {}", s.label()),
            };
            let target = if cfg.is_http() {
                cfg.url.clone().unwrap_or_default()
            } else {
                cfg.command.clone()
            };
            let scope = scopes.get(name).copied().unwrap_or("session");
            out.push_str(&format!(
                "  - {} [{}] {} ({}{}) {}\n",
                name,
                scope,
                cfg.transport_label(),
                state,
                auth,
                target
            ));
        }
        out.push_str(
            "\nManage with /mcp enable|disable <name> [--project], /mcp auth <name>, /mcp logout <name>, /mcp reload.\nenable/disable/add change global config unless --project is given.\n",
        );
        out
    }

    async fn set_enabled(
        &self,
        params: McpToolInput,
        enabled: bool,
        ctx: &ToolContext,
    ) -> Result<ToolOutput> {
        let verb = if enabled { "enable" } else { "disable" };
        let name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for {verb}"))?;
        let scope = parse_scope(params.scope.as_deref())?;
        let path = crate::mcp::McpConfig::set_server_enabled(
            &name,
            enabled,
            scope,
            Self::project_dir(ctx).as_deref(),
        )?;
        let reload = self.reload_config(&ctx.session_id).await?;
        let project_dir = Self::project_dir(ctx);
        let effective = crate::mcp::McpConfig::list_configured(project_dir.as_deref())
            .into_iter()
            .find(|s| s.name == name);
        let saved = format!(
            "Saved {} {} for MCP server '{}' in {}.",
            scope.as_str(),
            if enabled { "enabled" } else { "disabled" },
            name,
            path.display()
        );
        let effect = match effective {
            Some(e) if e.enabled == enabled => format!(
                "Effective in this project: {} (from {} config).",
                if e.enabled { "enabled" } else { "disabled" },
                e.scope.as_str()
            ),
            Some(e) => format!(
                "Effective in this project: still {} because {} config overrides it. Use --{} to change that.",
                if e.enabled { "enabled" } else { "disabled" },
                e.scope.as_str(),
                e.scope.as_str()
            ),
            None => "Server is no longer in the effective config.".to_string(),
        };
        Ok(
            ToolOutput::new(format!("{saved}\n{effect}\n\n{}", reload.output))
                .with_title(format!("MCP: {verb}d {name}")),
        )
    }

    async fn add_remote(&self, params: McpToolInput, ctx: &ToolContext) -> Result<ToolOutput> {
        let name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for add"))?;
        let url = params
            .url
            .or(params.command)
            .ok_or_else(|| anyhow::anyhow!("'url' is required for add"))?;
        let scope = parse_scope(params.scope.as_deref())?;
        let path = crate::mcp::McpConfig::add_remote_server(
            &name,
            &url,
            HashMap::new(),
            scope,
            Self::project_dir(ctx).as_deref(),
            params.replace.unwrap_or(false),
        )?;
        let reload = self.reload_config(&ctx.session_id).await?;
        let needs_auth = self
            .configured_server(&name, ctx)
            .map(|cfg| {
                matches!(
                    crate::mcp::oauth::auth_status(&cfg),
                    crate::mcp::oauth::McpAuthStatus::NotAuthenticated
                        | crate::mcp::oauth::McpAuthStatus::Expired { refreshable: false }
                )
            })
            .unwrap_or(false);
        let hint = if needs_auth {
            format!("\nIf the server requires sign-in, run /mcp auth {name}.\n")
        } else {
            String::new()
        };
        Ok(ToolOutput::new(format!(
            "Added MCP server '{}' -> {} ({} scope, {}).\n{}\n{}",
            name,
            url,
            scope.as_str(),
            path.display(),
            hint,
            reload.output
        ))
        .with_title(format!("MCP: added {name}")))
    }

    async fn authenticate(&self, params: McpToolInput, ctx: &ToolContext) -> Result<ToolOutput> {
        let name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for auth"))?;
        let config = self
            .configured_server(&name, ctx)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' is not configured"))?;
        if crate::mcp::oauth::auth_status(&config)
            == crate::mcp::oauth::McpAuthStatus::NotApplicable
        {
            return Ok(ToolOutput::new(format!(
                "MCP server '{name}' does not use OAuth (stdio server or static Authorization header)."
            ))
            .with_title("MCP: auth not applicable"));
        }

        // The flow blocks until the browser callback arrives (up to five
        // minutes), so run it in the background and return the URL now.
        let (url_tx, url_rx) = tokio::sync::oneshot::channel::<String>();
        let url_tx = std::sync::Mutex::new(Some(url_tx));
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<Result<()>>();
        let manager = Arc::clone(&self.manager);
        let registry = self.registry.as_ref().and_then(|r| r.upgrade());
        let session_id = ctx.session_id.clone();
        let server = name.clone();
        let key = pending_auth_key(&config);
        let task_key = key.clone();
        let (gen_tx, gen_rx) = tokio::sync::oneshot::channel::<u64>();
        let handle = tokio::spawn(async move {
            let result = crate::mcp::oauth::authenticate(&server, &config, |url| {
                if let Some(tx) = url_tx.lock().ok().and_then(|mut g| g.take()) {
                    let _ = tx.send(url.to_string());
                }
            })
            .await;
            let message = match &result {
                Ok(()) => match reconnect_after_auth(&manager, registry.as_ref(), &server, &config)
                    .await
                {
                    Ok(true) => format!("MCP server '{server}' authenticated and connected."),
                    Ok(false) => format!(
                        "MCP server '{server}' authenticated. It is disabled or its config changed, so it was not reconnected."
                    ),
                    Err(e) => {
                        format!("MCP server '{server}' authenticated, but reconnect failed: {e}")
                    }
                },
                Err(e) => format!("MCP authentication for '{server}' failed: {e}"),
            };
            if let Ok(generation) = gen_rx.await {
                forget_pending_auth(&task_key, generation);
            }
            crate::bus::Bus::global().publish(crate::bus::BusEvent::UiActivity(
                crate::bus::UiActivity::auth(Some(session_id), message.clone(), Some(message)),
            ));
            let _ = done_tx.send(result);
        });
        // A new auth (or logout) supersedes any in-flight flow for this server
        // so a late callback cannot store stale credentials.
        let _ = gen_tx.send(replace_pending_auth(&key, handle.abort_handle()));

        tokio::select! {
            url = url_rx => match url {
                Ok(url) => Ok(ToolOutput::new(format!(
                    "Open this URL to authenticate MCP server '{name}':\n\n{url}\n\n\
                    A browser window was opened if possible. jcode reconnects '{name}' \
                    automatically when sign-in completes (check with /mcp list)."
                ))
                .with_title(format!("MCP: auth {name}"))),
                Err(_) => match done_rx.await {
                    Ok(Ok(())) => Ok(ToolOutput::new(format!("MCP server '{name}' authenticated."))
                        .with_title(format!("MCP: auth {name}"))),
                    Ok(Err(e)) => Err(e),
                    Err(_) => anyhow::bail!("MCP authentication for '{name}' was cancelled"),
                },
            },
            _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                cancel_pending_auth(&key);
                anyhow::bail!("timed out preparing the authorization URL for '{name}'")
            }
        }
    }

    async fn logout(&self, params: McpToolInput, ctx: &ToolContext) -> Result<ToolOutput> {
        let name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for logout"))?;
        let config = self
            .configured_server(&name, ctx)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' is not configured"))?;
        let cancelled = cancel_pending_auth(&pending_auth_key(&config));
        let removed = crate::mcp::oauth::logout(&config)?;
        let connected = self.manager.read().await.connected_servers().await;
        if connected.contains(&name) {
            let _ = self
                .disconnect_server(McpToolInput {
                    action: "disconnect".into(),
                    server: Some(name.clone()),
                    command: None,
                    args: None,
                    env: None,
                    url: None,
                    scope: None,
                    replace: None,
                })
                .await;
        }
        let msg = if cancelled && !removed {
            format!("Cancelled pending OAuth sign-in for MCP server '{name}'.")
        } else if removed {
            format!("Removed stored OAuth credentials for MCP server '{name}' and disconnected it.")
        } else {
            format!("No stored OAuth credentials for MCP server '{name}'.")
        };
        Ok(ToolOutput::new(msg).with_title(format!("MCP: logout {name}")))
    }
}

type PendingAuthMap = HashMap<String, (u64, tokio::task::AbortHandle)>;

fn pending_auth() -> &'static std::sync::Mutex<PendingAuthMap> {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<PendingAuthMap>> =
        std::sync::OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// Pending flows are keyed by the credential identity (endpoint URL plus
/// OAuth client settings), not the server name, so identically named servers
/// in different repos or sessions never cancel each other.
fn pending_auth_key(config: &McpServerConfig) -> String {
    let oauth = config
        .oauth
        .as_ref()
        .and_then(|o| serde_json::to_string(o).ok())
        .unwrap_or_default();
    format!(
        "{}\u{1f}{}",
        config.url.as_deref().unwrap_or_default().trim(),
        oauth
    )
}

/// Register `handle`, aborting any flow it supersedes. Returns its generation.
fn replace_pending_auth(key: &str, handle: tokio::task::AbortHandle) -> u64 {
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut map) = pending_auth().lock()
        && let Some((_, old)) = map.insert(key.to_string(), (generation, handle))
    {
        old.abort();
    }
    generation
}

/// Abort an in-flight OAuth flow. Returns whether one was pending.
fn cancel_pending_auth(key: &str) -> bool {
    pending_auth()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(key))
        .map(|(_, h)| {
            h.abort();
            true
        })
        .unwrap_or(false)
}

/// Drop the entry only if it still belongs to `generation`, so a finishing
/// older flow cannot remove its replacement.
fn forget_pending_auth(key: &str, generation: u64) {
    if let Ok(mut map) = pending_auth().lock()
        && map.get(key).is_some_and(|(g, _)| *g == generation)
    {
        map.remove(key);
    }
}

/// Whether `fresh` still describes the endpoint that was authenticated.
fn same_remote_endpoint(authed: &McpServerConfig, fresh: &McpServerConfig) -> bool {
    authed.url == fresh.url && authed.transport == fresh.transport && authed.oauth == fresh.oauth
}

/// Reconnect a server after OAuth completes. Re-reads config under the manager
/// write guard (draining in-flight calls) and skips reconnecting when the
/// server was disabled, removed, or repointed during consent.
async fn reconnect_after_auth(
    manager: &Arc<RwLock<McpManager>>,
    registry: Option<&crate::tool::Registry>,
    name: &str,
    authed: &McpServerConfig,
) -> Result<bool> {
    let guard = manager.write().await;
    let fresh = guard.load_fresh_config();
    let Some(current) = fresh.servers.get(name).cloned() else {
        return Ok(false);
    };
    if !current.is_enabled() || !same_remote_endpoint(authed, &current) {
        return Ok(false);
    }
    if guard.connected_servers().await.iter().any(|s| s == name) {
        guard.disconnect(name).await?;
    }
    guard.connect(name, &current).await?;
    let tools = guard.all_tools().await;
    let connected = guard.connected_servers().await;
    drop(guard);
    if let Some(registry) = registry {
        registry
            .refresh_mcp_tools(
                crate::mcp::create_mcp_tools_from_cached_many(&tools, Arc::clone(manager)),
                &connected,
            )
            .await;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use std::fs;
    use std::path::PathBuf;

    fn create_test_tool() -> McpManagementTool {
        // Use an explicit empty config so tests are hermetic: McpManager::new()
        // would load the developer's real ~/.jcode/mcp.json, and list output
        // now includes configured-but-not-connected servers (issue #436).
        let manager = Arc::new(RwLock::new(McpManager::with_config(
            crate::mcp::McpConfig::default(),
        )));
        McpManagementTool::new(manager)
    }

    fn create_test_context() -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            tool_call_id: "test-tool-call".to_string(),
            working_dir: None,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: crate::tool::ToolExecutionMode::Direct,
        }
    }

    struct LocalMcpConfigGuard {
        path: PathBuf,
        backup: Option<String>,
        created_dir: bool,
    }

    impl LocalMcpConfigGuard {
        fn new(content: &str) -> std::io::Result<Self> {
            let path = PathBuf::from(".jcode/mcp.json");
            let dir = path
                .parent()
                .ok_or_else(|| std::io::Error::other("missing parent"))?;
            let created_dir = if !dir.exists() {
                fs::create_dir_all(dir)?;
                true
            } else {
                false
            };
            let backup = if path.exists() {
                Some(fs::read_to_string(&path)?)
            } else {
                None
            };
            fs::write(&path, content)?;
            Ok(Self {
                path,
                backup,
                created_dir,
            })
        }
    }

    impl Drop for LocalMcpConfigGuard {
        fn drop(&mut self) {
            match &self.backup {
                Some(content) => {
                    let _ = fs::write(&self.path, content);
                }
                None => {
                    let _ = fs::remove_file(&self.path);
                    if self.created_dir
                        && let Some(dir) = self.path.parent()
                    {
                        let _ = fs::remove_dir(dir);
                    }
                }
            }
        }
    }

    #[test]
    fn test_tool_name() {
        let tool = create_test_tool();
        assert_eq!(tool.name(), "mcp");
    }

    #[test]
    fn test_tool_description() {
        let tool = create_test_tool();
        assert!(tool.description().contains("MCP"));
        assert!(tool.description().contains("Model Context Protocol"));
    }

    #[test]
    fn test_parameters_schema() {
        let tool = create_test_tool();
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["server"].is_object());
        assert!(schema["properties"]["command"].is_object());
    }

    #[test]
    fn mcp_call_allows_dynamic_argument_keys_in_provider_schemas() {
        let tool = McpCallTool::new(Arc::clone(create_test_tool().manager()));
        let schema = tool.parameters_schema();
        assert_eq!(
            schema["properties"]["arguments"]["additionalProperties"],
            true
        );

        for spec in [
            &jcode_schema_dialect::registry::OPENROUTER,
            &jcode_schema_dialect::registry::OPENAI,
            &jcode_schema_dialect::registry::ANTHROPIC,
        ] {
            let normalized = jcode_schema_dialect::dialect::apply(&schema, spec);
            let arguments = &normalized["properties"]["arguments"];
            assert_eq!(arguments["type"], "object", "{}", spec.id);
            assert_eq!(arguments["additionalProperties"], true, "{}", spec.id);
            if spec.transforms.require_properties_on_objects {
                // Empty declared properties must not close the dynamic payload (#1214).
                assert_eq!(arguments["properties"], json!({}), "{}", spec.id);
            }
            assert_eq!(normalized["required"], schema["required"], "{}", spec.id);
        }
    }

    #[test]
    fn mcp_call_dynamic_arguments_remain_ineligible_for_openai_strict_mode() {
        let tool = McpCallTool::new(Arc::clone(create_test_tool().manager()));
        let compatible =
            jcode_provider_core::openai_schema::openai_compatible_schema(&tool.parameters_schema());
        assert!(!jcode_provider_core::openai_schema::schema_supports_strict(
            &compatible
        ));
        assert_eq!(
            compatible["properties"]["arguments"]["additionalProperties"],
            true
        );
    }

    #[tokio::test]
    async fn test_list_empty() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "list"});

        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.output.contains("No MCP servers connected"));
    }

    #[tokio::test]
    async fn test_list_shows_disabled_configured_server() {
        // Issue #436: disabled servers stay visible in the list with their
        // state, so users can see and enable them on demand.
        let mut config = crate::mcp::McpConfig::default();
        config.servers.insert(
            "off-server".to_string(),
            McpServerConfig {
                command: "some-bin".to_string(),
                args: vec![],
                env: HashMap::new(),
                shared: true,
                transport: None,
                url: None,
                headers: HashMap::new(),
                enabled: Some(false),
                disabled: None,
                timeout_secs: None,
                oauth: None,
            },
        );
        let manager = Arc::new(RwLock::new(McpManager::with_config(config)));
        let tool = McpManagementTool::new(manager);
        let ctx = create_test_context();

        let result = tool.execute(json!({"action": "list"}), ctx).await.unwrap();
        assert!(
            result.output.contains("off-server"),
            "disabled server must be listed: {}",
            result.output
        );
        assert!(
            result.output.contains("disabled in config"),
            "disabled state must be visible: {}",
            result.output
        );
    }

    fn remote(url: &str) -> McpServerConfig {
        serde_json::from_value(json!({"type": "http", "url": url})).unwrap()
    }

    #[test]
    fn pending_auth_key_is_per_endpoint_not_name() {
        let a = pending_auth_key(&remote("https://a.example/mcp"));
        let b = pending_auth_key(&remote("https://b.example/mcp"));
        assert_ne!(a, b);
        assert_eq!(a, pending_auth_key(&remote("https://a.example/mcp")));
    }

    #[tokio::test]
    async fn pending_auth_lifecycle_isolated_and_generation_safe() {
        let key_a = pending_auth_key(&remote("https://lifecycle-a.example/mcp"));
        let key_b = pending_auth_key(&remote("https://lifecycle-b.example/mcp"));
        let sleeper = || tokio::spawn(tokio::time::sleep(std::time::Duration::from_secs(60)));

        let a1 = sleeper();
        let g1 = replace_pending_auth(&key_a, a1.abort_handle());
        let b1 = sleeper();
        replace_pending_auth(&key_b, b1.abort_handle());

        // Re-auth of A aborts only the older A flow.
        let a2 = sleeper();
        let g2 = replace_pending_auth(&key_a, a2.abort_handle());
        assert!(a1.await.unwrap_err().is_cancelled());
        assert!(!b1.is_finished());

        // Stale completion must not drop the replacement.
        forget_pending_auth(&key_a, g1);
        assert!(cancel_pending_auth(&key_a));
        assert!(a2.await.unwrap_err().is_cancelled());
        forget_pending_auth(&key_a, g2);
        assert!(!cancel_pending_auth(&key_a));

        assert!(cancel_pending_auth(&key_b));
        assert!(b1.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn test_connect_missing_server() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "connect", "command": "/bin/test"});

        let result = tool.execute(input, ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("server"));
    }

    #[tokio::test]
    async fn test_connect_missing_command() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "connect", "server": "test"});

        let result = tool.execute(input, ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("command"));
    }

    #[tokio::test]
    async fn test_disconnect_not_connected() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "disconnect", "server": "nonexistent"});

        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.output.contains("not connected"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "invalid_action"});

        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.output.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_reload_empty_config() {
        let _guard =
            LocalMcpConfigGuard::new("{\"servers\":{}}").expect("create temporary .jcode/mcp.json");
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "reload"});

        let result = tool.execute(input, ctx).await.unwrap();
        // With config merging, global config may have servers.
        // If both are empty: "No servers found in config"
        // If global has servers: "Reloaded MCP config" (may show connection failures)
        assert!(
            result.output.contains("No servers")
                || result.output.contains("Empty config")
                || result.output.contains("Connected servers: 0")
                || result.output.contains("Reloaded MCP config")
        );
    }
}
