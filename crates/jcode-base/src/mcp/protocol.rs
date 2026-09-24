//! MCP Protocol types (JSON-RPC 2.0)

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC request
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC notification (a request without an `id`).
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC response
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

/// MCP Initialize params
#[derive(Debug, Clone, Serialize)]
pub struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ClientCapabilities {}

#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// MCP Initialize result
#[derive(Debug, Clone, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    pub server_info: Option<ServerInfo>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServerCapabilities {
    #[serde(default)]
    pub tools: Option<ToolsCapability>,
    #[serde(default)]
    pub resources: Option<ResourcesCapability>,
    #[serde(default)]
    pub prompts: Option<PromptsCapability>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolsCapability {
    #[serde(rename = "listChanged", default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResourcesCapability {
    #[serde(default)]
    pub subscribe: bool,
    #[serde(rename = "listChanged", default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PromptsCapability {
    #[serde(rename = "listChanged", default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// MCP Tool definition from server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// tools/list result
#[derive(Debug, Clone, Deserialize)]
pub struct ToolsListResult {
    pub tools: Vec<McpToolDef>,
}

/// tools/call params
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallParams {
    pub name: String,
    pub arguments: Value,
}

/// tools/call result
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallResult {
    pub content: Vec<ContentBlock>,
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

/// Content block in tool result
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    #[serde(rename = "resource")]
    Resource { resource: ResourceContent },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResourceContent {
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    pub text: Option<String>,
    pub blob: Option<String>,
}

/// MCP server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    /// Command for stdio servers. Empty for HTTP/SSE servers, which connected
    /// natively over HTTP.
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Whether this server can be shared across sessions (default: true).
    /// Stateless API wrappers (Todoist, Canvas) should be shared.
    /// Stateful servers (Playwright browser) should not be shared.
    #[serde(default = "default_shared")]
    pub shared: bool,
    /// Transport type ("stdio", "http"/"streamable-http", or legacy "sse",
    /// which is unsupported). Defaults to stdio, or http when only `url` is set.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    /// Endpoint URL for streamable-HTTP servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Extra HTTP headers for remote servers (env-expanded at load).
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub headers: std::collections::HashMap<String, String>,
    /// Whether this server is enabled (default: true). Disabled servers stay
    /// registered in config but are not spawned or connected at load time
    /// until re-enabled (issue #436). opencode-style `"enabled": false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Claude Code style alias: `"disabled": true`. Wins over `enabled` when
    /// both are present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Per-request reply timeout in seconds for this server (tools/call,
    /// tools/list, initialize). Absent keeps the default of 30s. Servers whose
    /// tools legitimately run long (multi-engine web search, browser fetch, PDF
    /// extraction) can raise it here (issues #802, #1174).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// OAuth settings for remote servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<super::oauth::McpOAuthConfig>,
}

/// Where a jcode-managed MCP config entry lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpConfigScope {
    /// `~/.jcode/mcp.json`
    Global,
    /// `<project root>/.jcode/mcp.json` (project root = nearest git root)
    Project,
}

impl McpConfigScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
        }
    }
}

impl std::str::FromStr for McpConfigScope {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "global" | "user" => Ok(Self::Global),
            "project" | "local" => Ok(Self::Project),
            other => anyhow::bail!("unknown MCP config scope '{other}' (use global or project)"),
        }
    }
}

/// One entry of the effective MCP configuration, including disabled servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpConfiguredServer {
    pub name: String,
    /// Project when any project-local config defines or overrides the entry.
    pub scope: McpConfigScope,
    pub enabled: bool,
    /// "stdio", "http" or "sse".
    pub transport: String,
    /// Command (stdio) or URL (remote), unexpanded.
    pub target: String,
    pub shared: bool,
}

impl McpServerConfig {
    /// A config entry is stdio when it has a command and is not explicitly a
    /// remote (http/sse) transport.
    pub fn is_stdio(&self) -> bool {
        if self.is_remote_transport() {
            return false;
        }
        !self.command.trim().is_empty()
    }

    fn is_remote_transport(&self) -> bool {
        self.transport.as_deref().is_some_and(|t| {
            matches!(
                t.to_ascii_lowercase().as_str(),
                "http" | "sse" | "streamable-http" | "streamable_http" | "remote"
            )
        })
    }

    /// Legacy HTTP+SSE transport, which jcode does not implement.
    pub fn is_legacy_sse(&self) -> bool {
        self.transport
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("sse"))
    }

    /// Native streamable-HTTP server: has a URL, is not a stdio command entry
    /// and is not the unsupported legacy SSE transport.
    pub fn is_http(&self) -> bool {
        if self.is_legacy_sse() {
            return false;
        }
        let has_url = self.url.as_deref().is_some_and(|u| !u.trim().is_empty());
        has_url && (self.is_remote_transport() || self.command.trim().is_empty())
    }

    /// Whether jcode can connect to this entry (stdio or native HTTP).
    pub fn is_runnable(&self) -> bool {
        self.is_stdio() || self.is_http()
    }

    /// Transport label for display.
    pub fn transport_label(&self) -> &'static str {
        if self.is_legacy_sse() {
            "sse"
        } else if self.is_http() {
            "http"
        } else {
            "stdio"
        }
    }

    /// An entry with neither command nor URL, used by project configs to only
    /// toggle `enabled`/`disabled` for a server defined elsewhere.
    pub fn is_enablement_override(&self) -> bool {
        self.command.trim().is_empty() && self.url.as_deref().is_none_or(|u| u.trim().is_empty())
    }

    /// Whether this server should be spawned/connected automatically.
    /// Defaults to true. `"disabled": true` (Claude Code style) wins over
    /// `"enabled"` (opencode style) when both are present. Disabled servers
    /// stay in config and can still be connected on demand by name.
    pub fn is_enabled(&self) -> bool {
        if let Some(disabled) = self.disabled {
            return !disabled;
        }
        self.enabled.unwrap_or(true)
    }
}

fn default_shared() -> bool {
    true
}

/// Full MCP configuration file
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct McpConfig {
    /// Server map. Accepts the canonical Claude Code key `mcpServers` as well as
    /// jcode's historical `servers` key.
    #[serde(default, alias = "mcpServers")]
    pub servers: std::collections::HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UnresolvedEnvironmentVariable {
    server: String,
    variable: String,
}

fn valid_environment_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|ch| matches!(ch, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

/// Expand Claude Code's documented `${VAR}` and `${VAR:-default}` syntax in a
/// single config string. Unsupported/malformed expressions are preserved.
fn expand_environment_string<F>(
    value: &str,
    lookup: &F,
    unresolved: &mut std::collections::BTreeSet<String>,
) -> String
where
    F: Fn(&str) -> Option<String>,
{
    let mut output = String::with_capacity(value.len());
    let mut remainder = value;

    while let Some(start) = remainder.find("${") {
        output.push_str(&remainder[..start]);
        let expression_start = start + 2;
        let Some(relative_end) = remainder[expression_start..].find('}') else {
            output.push_str(&remainder[start..]);
            return output;
        };
        let end = expression_start + relative_end;
        let expression = &remainder[expression_start..end];
        let (variable, default) = match expression.split_once(":-") {
            Some((variable, default)) => (variable, Some(default)),
            None => (expression, None),
        };
        let literal = &remainder[start..=end];

        if !valid_environment_variable_name(variable) {
            output.push_str(literal);
        } else if let Some(expanded) = lookup(variable) {
            output.push_str(&expanded);
        } else if let Some(default) = default {
            output.push_str(default);
        } else {
            unresolved.insert(variable.to_string());
            output.push_str(literal);
        }

        remainder = &remainder[end + 1..];
    }

    output.push_str(remainder);
    output
}

impl McpConfig {
    /// Load config from file
    pub fn load_from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Save config to a JSON file
    pub fn save_to_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Expand environment references only after all config sources have been
    /// merged. This avoids warning about shadowed definitions and ensures every
    /// downstream consumer, including the tool-schema cache, sees the exact
    /// values that will be passed to the MCP process.
    fn expand_environment_variables_with<F>(
        &mut self,
        lookup: F,
    ) -> Vec<UnresolvedEnvironmentVariable>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut warnings = Vec::new();

        for (server_name, config) in &mut self.servers {
            let mut unresolved = std::collections::BTreeSet::new();
            config.command = expand_environment_string(&config.command, &lookup, &mut unresolved);
            for arg in &mut config.args {
                *arg = expand_environment_string(arg, &lookup, &mut unresolved);
            }
            for value in config.env.values_mut() {
                *value = expand_environment_string(value, &lookup, &mut unresolved);
            }
            if let Some(url) = &mut config.url {
                *url = expand_environment_string(url, &lookup, &mut unresolved);
            }
            for value in config.headers.values_mut() {
                *value = expand_environment_string(value, &lookup, &mut unresolved);
            }

            warnings.extend(
                unresolved
                    .into_iter()
                    .map(|variable| UnresolvedEnvironmentVariable {
                        server: server_name.clone(),
                        variable,
                    }),
            );
        }

        warnings.sort();
        warnings
    }

    fn expand_environment_variables(&mut self) {
        let warnings =
            self.expand_environment_variables_with(|variable| std::env::var(variable).ok());
        for warning in warnings {
            crate::logging::warn(&format!(
                "MCP: Server '{}' references unset environment variable '{}'; leaving '${{{}}}' unexpanded",
                warning.server, warning.variable, warning.variable
            ));
        }
    }

    /// Import MCP servers from Codex CLI on first run.
    ///
    /// Claude Code configuration is intentionally not imported here. It is a
    /// live source read by `load_for_dir`, so persisting it would make deleted
    /// servers survive in jcode's snapshot and would duplicate inline secrets.
    /// This only runs while ~/.jcode/mcp.json does not exist.
    fn import_from_codex_once() {
        let jcode_mcp = match crate::storage::jcode_dir() {
            Ok(dir) => dir.join("mcp.json"),
            Err(_) => return,
        };

        if jcode_mcp.exists() {
            return; // Not first run
        }

        let Ok(codex_config) = crate::storage::user_home_path(".codex/config.toml") else {
            return;
        };
        if !codex_config.exists() {
            return;
        }
        let Ok(imported) = Self::load_from_codex_toml(&codex_config) else {
            return;
        };
        if imported.servers.is_empty() {
            return;
        }

        let server_count = imported.servers.len();
        let environment_value_count = imported
            .servers
            .values()
            .map(|server| server.env.len())
            .sum();
        if let Err(e) = imported.save_to_file(&jcode_mcp) {
            crate::logging::error(&format!("Failed to save imported MCP config: {}", e));
            return;
        }
        crate::logging::info(&Self::codex_import_log_message(
            server_count,
            environment_value_count,
            &jcode_mcp,
        ));
    }

    fn codex_import_log_message(
        server_count: usize,
        environment_value_count: usize,
        destination: &std::path::Path,
    ) -> String {
        let environment_note = if environment_value_count == 0 {
            "no configured environment values were copied".to_string()
        } else {
            format!(
                "copied {} configured environment value(s), which may contain secrets",
                environment_value_count
            )
        };
        format!(
            "MCP: One-time imported {} server(s) from Codex CLI (~/.codex/config.toml) into {}; {}. Claude Code MCP configuration remains live and was not copied",
            server_count,
            destination.display(),
            environment_note,
        )
    }

    fn live_claude_log_message(server_count: usize, source: &str) -> String {
        format!(
            "MCP: Loaded {} server(s) live from Claude Code ({}); source values were not copied into jcode config",
            server_count, source
        )
    }

    /// Parse MCP servers from Codex CLI's config.toml ([mcp_servers.*] sections)
    fn load_from_codex_toml(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let table: toml::Table = content.parse()?;

        let mut config = Self::default();
        if let Some(toml::Value::Table(mcp_servers)) = table.get("mcp_servers") {
            for (name, value) in mcp_servers {
                if let toml::Value::Table(server) = value {
                    let command = server
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if command.is_empty() {
                        continue;
                    }
                    let args = server
                        .get("args")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let env = server
                        .get("env")
                        .and_then(|v| v.as_table())
                        .map(|t| {
                            t.iter()
                                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                .collect()
                        })
                        .unwrap_or_default();
                    let shared = server
                        .get("shared")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    // Codex uses `enabled = false` to keep a server registered
                    // but not started; carrying it over keeps disabled servers
                    // disabled after the one-time import instead of silently
                    // activating them.
                    let enabled = server.get("enabled").and_then(|v| v.as_bool());
                    config.servers.insert(
                        name.clone(),
                        McpServerConfig {
                            command,
                            args,
                            env,
                            shared,
                            transport: None,
                            url: None,
                            headers: std::collections::HashMap::new(),
                            enabled,
                            disabled: None,
                            timeout_secs: None,
                            oauth: None,
                        },
                    );
                }
            }
        }
        Ok(config)
    }

    /// Parse MCP servers from Claude Code's `~/.claude.json`.
    ///
    /// Claude Code stores a global set under the top-level `mcpServers` key, and
    /// per-project sets under `projects.<abs_path>.mcpServers`. We merge the
    /// global set first, then overlay the entry for `cwd` (if any) so a
    /// project-specific server wins for the active directory.
    fn load_claude_json(path: &std::path::Path, cwd: Option<&std::path::Path>) -> Self {
        let mut config = Self::default();
        let Ok(content) = std::fs::read_to_string(path) else {
            return config;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return config;
        };

        // Global servers under top-level `mcpServers`.
        if let Some(map) = value.get("mcpServers")
            && let Ok(servers) = serde_json::from_value::<
                std::collections::HashMap<String, McpServerConfig>,
            >(map.clone())
        {
            config.servers.extend(servers);
        }

        // Per-project servers under `projects.<abs_path>.mcpServers`.
        if let (Some(cwd), Some(projects)) =
            (cwd, value.get("projects").and_then(|p| p.as_object()))
        {
            let cwd_str = cwd.to_string_lossy();
            if let Some(project) = projects.get(cwd_str.as_ref())
                && let Some(map) = project.get("mcpServers")
                && let Ok(servers) = serde_json::from_value::<
                    std::collections::HashMap<String, McpServerConfig>,
                >(map.clone())
            {
                Self::merge_servers_preferring_runnable(&mut config.servers, servers);
            }
        }

        config
    }

    /// Only the `projects.<dir>.mcpServers` entries of `~/.claude.json`.
    fn load_claude_project_entries(path: &std::path::Path, dir: &std::path::Path) -> Self {
        let mut config = Self::default();
        let Ok(content) = std::fs::read_to_string(path) else {
            return config;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return config;
        };
        if let Some(map) = value
            .get("projects")
            .and_then(|p| p.get(dir.to_string_lossy().as_ref()))
            .and_then(|p| p.get("mcpServers"))
            && let Ok(servers) = serde_json::from_value(map.clone())
        {
            config.servers = servers;
        }
        config
    }

    /// Load project-local MCP config files from `project_root`, in override
    /// order: `.claude/mcp.json` (legacy), then `.mcp.json` (Claude Code
    /// project config), then `.jcode/mcp.json` (jcode-managed, wins).
    fn load_project_locals(project_root: &std::path::Path) -> Self {
        let mut merged = Self::default();
        for relative in [".claude/mcp.json", ".mcp.json", ".jcode/mcp.json"] {
            let path = project_root.join(relative);
            if path.exists()
                && let Ok(config) = Self::load_from_file(&path)
            {
                Self::merge_servers_preferring_runnable(&mut merged.servers, config.servers);
            }
        }
        merged
    }

    /// Load from default locations (merges jcode global + local, local overrides),
    /// resolving project-local config against the process working directory.
    pub fn load() -> Self {
        let cwd = std::env::current_dir().ok();
        Self::load_for_dir(cwd.as_deref())
    }

    /// Load from default locations, resolving project-local config
    /// (`.jcode/mcp.json`, `.mcp.json`, `.claude/mcp.json`, and the per-project
    /// entries in `~/.claude.json`) against `project_dir` instead of the
    /// process working directory when provided.
    ///
    /// Remote/client sessions run inside a long-lived server whose cwd is
    /// unrelated to the session's project, so the session working directory
    /// must be threaded through explicitly (issue #420).
    #[expect(
        clippy::collapsible_if,
        reason = "Import logic keeps source-specific MCP config merge order explicit"
    )]
    pub fn load_for_dir(project_dir: Option<&std::path::Path>) -> Self {
        // Codex CLI is a one-time migration. Claude Code remains a live source.
        Self::import_from_codex_once();

        let (mut merged, _) = Self::load_unexpanded(project_dir);

        // Claude Code expands environment references after source precedence is
        // resolved, so URLs and headers of remote servers are expanded too.
        merged.expand_environment_variables();
        merged
    }

    /// Merge all sources without env expansion. Returns the merged config and
    /// the set of names defined or overridden by project-local files.
    #[expect(
        clippy::collapsible_if,
        reason = "Import logic keeps source-specific MCP config merge order explicit"
    )]
    fn load_unexpanded(
        project_dir: Option<&std::path::Path>,
    ) -> (Self, std::collections::HashSet<String>) {
        // Precedence (later wins): imported global sources, then jcode's
        // global config, then imported project sources, then the project's
        // .jcode/mcp.json. Explicit jcode management wins within each tier and
        // project still wins over global. Enablement-only entries in jcode
        // files inherit the definition they override.
        let mut merged = Self::default();
        let claude_mcp_enabled = std::env::var_os("JCODE_DISABLE_CLAUDE_MCP").is_none();
        let claude_json = claude_mcp_enabled
            .then(|| crate::storage::user_home_path(".claude.json").ok())
            .flatten()
            .filter(|p| p.exists());

        // Imported global: ~/.claude.json top-level, then legacy ~/.claude/mcp.json.
        if let Some(path) = &claude_json {
            let config = Self::load_claude_json(path, None);
            if !config.servers.is_empty() {
                crate::logging::info(&Self::live_claude_log_message(
                    config.servers.len(),
                    "~/.claude.json",
                ));
            }
            Self::merge_servers_preferring_runnable(&mut merged.servers, config.servers);
        }
        if claude_mcp_enabled
            && let Ok(claude_mcp) = crate::storage::user_home_path(".claude/mcp.json")
            && claude_mcp.exists()
            && let Ok(config) = Self::load_from_file(&claude_mcp)
        {
            if !config.servers.is_empty() {
                crate::logging::info(&Self::live_claude_log_message(
                    config.servers.len(),
                    "~/.claude/mcp.json (legacy)",
                ));
            }
            Self::merge_servers_preferring_runnable(&mut merged.servers, config.servers);
        }

        // jcode global (~/.jcode/mcp.json).
        if let Ok(path) = Self::config_path(McpConfigScope::Global, None)
            && path.exists()
            && let Ok(config) = Self::load_from_file(&path)
        {
            Self::merge_servers_preferring_runnable(&mut merged.servers, config.servers);
        }

        let mut project_names = std::collections::HashSet::new();
        if let Some(dir) = project_dir {
            // Imported project: ~/.claude.json per-project entries.
            if let Some(path) = &claude_json {
                let config = Self::load_claude_project_entries(path, dir);
                project_names.extend(config.servers.keys().cloned());
                Self::merge_servers_preferring_runnable(&mut merged.servers, config.servers);
            }
            // Project-local files at the git root, then the exact dir.
            let root = Self::project_root(dir);
            let mut dirs = vec![root.clone()];
            if root != dir {
                dirs.push(dir.to_path_buf());
            }
            for d in dirs {
                let locals = Self::load_project_locals(&d).servers;
                project_names.extend(locals.keys().cloned());
                Self::merge_servers_preferring_runnable(&mut merged.servers, locals);
            }
        }

        // Drop entries jcode cannot connect to (e.g. an enablement-only
        // override whose base definition does not exist).
        merged.servers.retain(|name, cfg| {
            let keep = cfg.is_runnable();
            if !keep {
                crate::logging::info(&format!(
                    "MCP: Skipping server '{}': {}",
                    name,
                    if cfg.is_legacy_sse() {
                        "legacy SSE transport is not supported (use streamable HTTP)"
                    } else {
                        "no command or url configured"
                    }
                ));
            }
            keep
        });

        (merged, project_names)
    }

    /// Nearest ancestor of `dir` containing `.git`, else `dir` itself.
    pub fn project_root(dir: &std::path::Path) -> std::path::PathBuf {
        dir.ancestors()
            .find(|a| a.join(".git").exists())
            .unwrap_or(dir)
            .to_path_buf()
    }

    /// Path of the jcode-managed config file for `scope`.
    pub fn config_path(
        scope: McpConfigScope,
        project_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<std::path::PathBuf> {
        match scope {
            McpConfigScope::Global => Ok(crate::storage::jcode_dir()?.join("mcp.json")),
            McpConfigScope::Project => {
                let dir = match project_dir {
                    Some(d) => d.to_path_buf(),
                    None => std::env::current_dir()?,
                };
                Ok(Self::project_root(&dir).join(".jcode").join("mcp.json"))
            }
        }
    }

    /// Effective configured servers (including disabled ones), sorted by name.
    pub fn list_configured(project_dir: Option<&std::path::Path>) -> Vec<McpConfiguredServer> {
        let (merged, project_names) = Self::load_unexpanded(project_dir);
        let mut out: Vec<_> = merged
            .servers
            .iter()
            .map(|(name, cfg)| McpConfiguredServer {
                name: name.clone(),
                scope: if project_names.contains(name) {
                    McpConfigScope::Project
                } else {
                    McpConfigScope::Global
                },
                enabled: cfg.is_enabled(),
                transport: cfg.transport_label().to_string(),
                target: if cfg.is_http() || cfg.is_legacy_sse() {
                    cfg.url.clone().unwrap_or_default()
                } else {
                    cfg.command.clone()
                },
                shared: cfg.shared,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn read_raw(path: &std::path::Path) -> anyhow::Result<serde_json::Value> {
        if !path.exists() {
            return Ok(serde_json::json!({}));
        }
        let content = std::fs::read_to_string(path)?;
        if content.trim().is_empty() {
            return Ok(serde_json::json!({}));
        }
        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("invalid JSON in {}: {e}", path.display()))?;
        anyhow::ensure!(value.is_object(), "{} is not a JSON object", path.display());
        Ok(value)
    }

    fn write_raw(path: &std::path::Path, value: &serde_json::Value) -> anyhow::Result<()> {
        use std::io::Write;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid MCP config path {}", path.display()))?;
        std::fs::create_dir_all(parent)?;
        // Resolve symlinks so we replace the real file, not the link.
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let target_dir = target.parent().unwrap_or(parent);
        let mut tmp = tempfile::NamedTempFile::new_in(target_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Preserve existing permissions; new files are owner-only since
            // they may contain secrets in headers/env.
            let mode = std::fs::metadata(&target)
                .map(|m| m.permissions().mode() & 0o7777)
                .unwrap_or(0o600);
            std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(mode))?;
        }
        tmp.write_all((serde_json::to_string_pretty(value)? + "\n").as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(&target).map_err(|e| e.error)?;
        Ok(())
    }

    /// Serialize read-modify-write of a managed config: in-process mutex plus
    /// an advisory flock on a sidecar lock file (cross-process, unix).
    fn with_config_lock<T>(
        path: &std::path::Path,
        f: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("json.lock"))?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            // SAFETY: valid open fd for the lifetime of lock_file.
            unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX) };
        }
        let result = f();
        drop(lock_file); // closing the fd releases the flock
        result
    }

    /// The server map object in a raw config, preferring an existing
    /// `mcpServers` or `servers` key and creating `servers` otherwise.
    fn raw_servers_mut(
        value: &mut serde_json::Value,
    ) -> anyhow::Result<&mut serde_json::Map<String, serde_json::Value>> {
        let obj = value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("MCP config is not a JSON object"))?;
        let key = if obj.contains_key("mcpServers") {
            "mcpServers"
        } else {
            "servers"
        };
        obj.entry(key)
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("MCP config '{key}' is not a JSON object"))
    }

    /// Enable or disable `name` in the given scope, editing only the
    /// `enabled`/`disabled` keys so unknown keys and `${VAR}` expressions are
    /// preserved. Project scope may create an enablement-only override for a
    /// server defined globally. Returns the file written.
    pub fn set_server_enabled(
        name: &str,
        enabled: bool,
        scope: McpConfigScope,
        project_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<std::path::PathBuf> {
        let path = Self::config_path(scope, project_dir)?;
        let known = Self::list_configured(project_dir)
            .iter()
            .any(|s| s.name == name);
        Self::with_config_lock(&path, || {
            let mut raw = Self::read_raw(&path)?;
            let servers = Self::raw_servers_mut(&mut raw)?;
            match servers.get_mut(name) {
                Some(entry) => {
                    let obj = entry.as_object_mut().ok_or_else(|| {
                        anyhow::anyhow!("MCP server '{name}' entry is not an object")
                    })?;
                    obj.remove("disabled");
                    obj.insert("enabled".into(), serde_json::Value::Bool(enabled));
                }
                None => {
                    anyhow::ensure!(known, "MCP server '{name}' is not configured");
                    // Enablement-only override for a server defined elsewhere
                    // (imported Claude config or the global jcode config).
                    servers.insert(name.to_string(), serde_json::json!({ "enabled": enabled }));
                }
            }
            Self::write_raw(&path, &raw)
        })?;
        Ok(path)
    }

    /// Add a remote streamable-HTTP MCP server in the given scope. Refuses an
    /// existing name in that file unless `replace` is true.
    pub fn add_remote_server(
        name: &str,
        url: &str,
        headers: std::collections::HashMap<String, String>,
        scope: McpConfigScope,
        project_dir: Option<&std::path::Path>,
        replace: bool,
    ) -> anyhow::Result<std::path::PathBuf> {
        let name = name.trim();
        anyhow::ensure!(
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
            "invalid MCP server name '{name}'"
        );
        let url = url.trim();
        if !url.contains("${") {
            super::oauth::validate_endpoint(url)?;
        } else {
            anyhow::ensure!(
                url.starts_with("https://") || url.starts_with("http://") || url.starts_with("${"),
                "MCP server URL must start with http:// or https://"
            );
        }
        let path = Self::config_path(scope, project_dir)?;
        Self::with_config_lock(&path, || {
            let mut raw = Self::read_raw(&path)?;
            let servers = Self::raw_servers_mut(&mut raw)?;
            anyhow::ensure!(
                replace || !servers.contains_key(name),
                "MCP server '{name}' already exists in {}; pass replace to overwrite it",
                path.display()
            );
            let mut entry = serde_json::json!({ "type": "http", "url": url });
            if !headers.is_empty() {
                entry["headers"] = serde_json::to_value(headers)?;
            }
            servers.insert(name.to_string(), entry);
            Self::write_raw(&path, &raw)
        })?;
        Ok(path)
    }

    /// Merge `incoming` over `existing`, except that an entry jcode cannot run
    /// (HTTP/SSE) never displaces a working stdio entry for the same name.
    ///
    /// Without this, a `type: http` entry in `~/.claude.json` would overwrite a
    /// working stdio server from `~/.jcode/mcp.json` and then be dropped by the
    /// non-stdio filter, silently losing the server (issue #653).
    fn merge_servers_preferring_runnable(
        existing: &mut std::collections::HashMap<String, McpServerConfig>,
        incoming: std::collections::HashMap<String, McpServerConfig>,
    ) {
        for (name, cfg) in incoming {
            if cfg.is_enablement_override() {
                if let Some(current) = existing.get_mut(&name) {
                    if cfg.enabled.is_some() || cfg.disabled.is_some() {
                        current.enabled = Some(cfg.is_enabled());
                        current.disabled = None;
                    }
                    continue;
                }
            }
            // A runnable definition is never displaced by one jcode cannot
            // connect to (e.g. legacy SSE), issue #653. Native HTTP entries
            // follow normal source precedence.
            if let Some(current) = existing.get(&name)
                && current.is_runnable()
                && !cfg.is_runnable()
            {
                crate::logging::info(&format!(
                    "MCP: Keeping existing stdio server '{}'; ignoring {} definition from a lower-precedence config",
                    name,
                    cfg.transport.as_deref().unwrap_or("http")
                ));
                continue;
            }
            existing.insert(name, cfg);
        }
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod protocol_tests;
