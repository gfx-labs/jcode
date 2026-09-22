//! Native `/mcp` management commands.
//!
//! These execute the `mcp` management tool directly (locally against the
//! in-process registry, or on the server via `Request::McpCommand`) so they
//! never require an LLM turn. Every mutation reloads the MCP manager and
//! refreshes the active tool registry.

use super::{App, DisplayMessage};
use serde_json::{Value, json};

pub(super) const MCP_USAGE: &str = "/mcp [list|status]\n\
/mcp add <name> <url> [--project] [--replace] add a remote HTTP server (e.g. /mcp add figma https://mcp.figma.com/mcp)\n\
/mcp enable <name> [--project]\n\
/mcp disable <name> [--project]\n\
/mcp auth <name>                    sign in to an OAuth server (opens browser)\n\
/mcp logout <name>                  remove stored OAuth credentials\n\
/mcp connect <name> | /mcp disconnect <name>\n\
/mcp reload                         reload config and reconnect\n\
Scope: enable/disable/add write GLOBAL config (~/.jcode/mcp.json) unless --project is given,\n\
which writes <repo>/.jcode/mcp.json.";

/// Parse `/mcp ...` into `mcp` tool input. `Ok(None)` means not an /mcp command.
pub(super) fn parse_mcp_command(trimmed: &str) -> Option<Result<Value, String>> {
    let rest = trimmed.strip_prefix("/mcp")?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    let mut scope: Option<&str> = None;
    let mut replace = false;
    let mut words = Vec::new();
    let usage = |msg: String| Some(Err(format!("{msg}\n\nUsage:\n{MCP_USAGE}")));
    for word in rest.split_whitespace() {
        let flag_scope = match word {
            "--project" | "-p" | "--scope=project" => Some("project"),
            "--global" | "-g" | "--scope=global" => Some("global"),
            "--replace" if !replace => {
                replace = true;
                None
            }
            _ if word.starts_with('-') => {
                return usage(format!("Unknown /mcp option '{word}'."));
            }
            _ => {
                words.push(word);
                None
            }
        };
        if let Some(new) = flag_scope {
            if scope.is_some_and(|old| old != new) {
                return usage("Use only one of --project or --global.".to_string());
            }
            scope = Some(new);
        }
    }
    let action = words.first().copied().unwrap_or("list");
    let takes_scope = matches!(action, "enable" | "disable" | "add");
    if replace && action != "add" {
        return usage("--replace only applies to /mcp add.".to_string());
    }
    if scope.is_some() && !takes_scope {
        return usage(format!("/mcp {action} does not take --project/--global."));
    }
    let expected_args = match action {
        "add" => 3,
        "enable" | "disable" | "auth" | "login" | "logout" | "connect" | "disconnect" => 2,
        _ => 1,
    };
    if words.len() > expected_args {
        return usage(format!(
            "Unexpected extra argument '{}' for /mcp {action}.",
            words[expected_args]
        ));
    }
    let arg = words.get(1).copied();
    let need = |what: &str| format!("Missing {what}.\n\nUsage:\n{MCP_USAGE}");
    let mut input = match action {
        "list" | "status" | "ls" => json!({"action": "list"}),
        "reload" | "refresh" => json!({"action": "reload"}),
        "help" => return Some(Err(format!("Usage:\n{MCP_USAGE}"))),
        "enable" | "disable" | "auth" | "login" | "logout" | "connect" | "disconnect" => {
            let Some(name) = arg else {
                return Some(Err(need("server name")));
            };
            let action = if action == "login" { "auth" } else { action };
            json!({"action": action, "server": name})
        }
        "add" => {
            let (Some(name), Some(url)) = (arg, words.get(2)) else {
                return Some(Err(need("server name and URL")));
            };
            let mut v = json!({"action": "add", "server": name, "url": url});
            if replace {
                v["replace"] = json!(true);
            }
            v
        }
        other => {
            return Some(Err(format!(
                "Unknown /mcp subcommand '{other}'.\n\nUsage:\n{MCP_USAGE}"
            )));
        }
    };
    if let Some(scope) = scope {
        input["scope"] = json!(scope);
    }
    Some(Ok(input))
}

impl App {
    /// Tab-completion candidates for `/mcp ...`, including configured
    /// server names for subcommands that take one.
    pub(super) fn mcp_suggestions(prefix: &str) -> Vec<(String, &'static str)> {
        mcp_suggestions_for(prefix, &configured_server_names())
    }
}

fn configured_server_names() -> Vec<String> {
    let dir = std::env::current_dir().ok();
    crate::mcp::McpConfig::list_configured(dir.as_deref())
        .into_iter()
        .map(|s| s.name)
        .collect()
}

pub(super) fn mcp_suggestions_for(prefix: &str, servers: &[String]) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = vec![
        (
            "/mcp list".into(),
            "Show MCP servers, scope, status, and auth",
        ),
        (
            "/mcp add".into(),
            "Add a remote HTTP MCP server: /mcp add <name> <url>",
        ),
        (
            "/mcp add figma https://mcp.figma.com/mcp".into(),
            "Add the Figma remote MCP server",
        ),
        (
            "/mcp enable".into(),
            "Enable a server (add --project for this repo)",
        ),
        (
            "/mcp disable".into(),
            "Disable a server (add --project for this repo)",
        ),
        ("/mcp auth".into(), "Sign in to an OAuth MCP server"),
        ("/mcp logout".into(), "Remove stored OAuth credentials"),
        ("/mcp reload".into(), "Reload MCP config and reconnect"),
    ];
    let words: Vec<&str> = prefix.split_whitespace().collect();
    if let Some(&sub) = words.get(1)
        && matches!(
            sub,
            "enable" | "disable" | "auth" | "login" | "logout" | "connect" | "disconnect"
        )
    {
        let desc: &'static str = match sub {
            "enable" => "Enable this server",
            "disable" => "Disable this server",
            "logout" => "Remove stored credentials",
            "connect" => "Connect this server",
            "disconnect" => "Disconnect this server",
            _ => "Authenticate this server",
        };
        for name in servers {
            out.push((format!("/mcp {sub} {name}"), desc));
            if matches!(sub, "enable" | "disable") {
                out.push((
                    format!("/mcp {sub} {name} --project"),
                    "Only for this project",
                ));
            }
        }
    }
    out
}

fn mcp_tool_call(input: Value) -> crate::message::ToolCall {
    crate::message::ToolCall {
        id: crate::id::new_id("call"),
        name: "mcp".to_string(),
        input,
        intent: None,
        thought_signature: None,
    }
}

/// Local (in-process) `/mcp` handler.
pub(super) fn handle_mcp_command(app: &mut App, trimmed: &str) -> bool {
    let Some(parsed) = parse_mcp_command(trimmed) else {
        return false;
    };
    match parsed {
        Ok(_) if app.is_processing => app.push_display_message(DisplayMessage::system(
            "Wait for the current turn to finish before running /mcp.".to_string(),
        )),
        Ok(input) => {
            app.set_status_notice("Running /mcp");
            super::commands::launch_manual_tool_call(app, mcp_tool_call(input));
        }
        Err(message) => app.push_display_message(DisplayMessage::system(message)),
    }
    true
}

/// Remote `/mcp` handler: forwards to the server session so its registry is
/// the one refreshed.
pub(super) async fn handle_remote_mcp_command(
    app: &mut App,
    remote: &mut crate::tui::backend::RemoteConnection,
    trimmed: &str,
) -> anyhow::Result<bool> {
    let Some(parsed) = parse_mcp_command(trimmed) else {
        return Ok(false);
    };
    match parsed {
        Ok(_) if app.is_processing => app.push_display_message(DisplayMessage::system(
            "Wait for the current turn to finish before running /mcp.".to_string(),
        )),
        Ok(input) => {
            app.set_status_notice("Running /mcp");
            remote.run_mcp_command(input).await?;
        }
        Err(message) => app.push_display_message(DisplayMessage::system(message)),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> Value {
        parse_mcp_command(s).expect("is /mcp").expect("parses")
    }

    #[test]
    fn parses_list_default_and_status() {
        assert_eq!(ok("/mcp"), json!({"action": "list"}));
        assert_eq!(ok("/mcp status"), json!({"action": "list"}));
        assert!(parse_mcp_command("/mcpx").is_none());
        assert!(parse_mcp_command("/model").is_none());
    }

    #[test]
    fn parses_figma_workflow() {
        assert_eq!(
            ok("/mcp add figma https://mcp.figma.com/mcp"),
            json!({"action": "add", "server": "figma", "url": "https://mcp.figma.com/mcp"})
        );
        assert_eq!(
            ok("/mcp auth figma"),
            json!({"action": "auth", "server": "figma"})
        );
        assert_eq!(
            ok("/mcp login figma"),
            json!({"action": "auth", "server": "figma"})
        );
        assert_eq!(
            ok("/mcp disable figma --project"),
            json!({"action": "disable", "server": "figma", "scope": "project"})
        );
        assert_eq!(
            ok("/mcp enable --global figma"),
            json!({"action": "enable", "server": "figma", "scope": "global"})
        );
        assert_eq!(
            ok("/mcp logout figma"),
            json!({"action": "logout", "server": "figma"})
        );
        assert_eq!(ok("/mcp reload"), json!({"action": "reload"}));
        assert_eq!(
            ok("/mcp add figma https://mcp.figma.com/mcp --replace"),
            json!({"action": "add", "server": "figma", "url": "https://mcp.figma.com/mcp", "replace": true})
        );
    }

    #[test]
    fn completion_includes_subcommands_and_server_names() {
        let servers = vec!["figma".to_string()];
        let base: Vec<String> = mcp_suggestions_for("/mcp ", &servers)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        for cmd in [
            "/mcp list",
            "/mcp add",
            "/mcp auth",
            "/mcp disable",
            "/mcp reload",
        ] {
            assert!(base.iter().any(|s| s == cmd), "missing {cmd}");
        }
        let named: Vec<String> = mcp_suggestions_for("/mcp disable f", &servers)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert!(named.iter().any(|s| s == "/mcp disable figma --project"));
        let auth: Vec<String> = mcp_suggestions_for("/mcp auth ", &servers)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert!(auth.iter().any(|s| s == "/mcp auth figma"));
    }

    #[test]
    fn help_topic_documents_mcp() {
        let app = crate::tui::app::tests::create_test_app();
        let help = app.command_help("mcp").expect("mcp help");
        assert!(help.contains("/mcp add <name> <url>"));
        assert!(help.contains("--project"));
        assert!(help.contains("/mcp auth"));
    }

    #[test]
    fn reports_usage_errors() {
        let err = parse_mcp_command("/mcp enable").unwrap().unwrap_err();
        assert!(err.contains("Missing server name"));
        let err = parse_mcp_command("/mcp add figma").unwrap().unwrap_err();
        assert!(err.contains("/mcp add <name> <url>"));
        assert!(parse_mcp_command("/mcp bogus").unwrap().is_err());
        for bad in [
            "/mcp disable figma --scope=projct",
            "/mcp disable figma --projet",
            "/mcp disable figma --project --global",
            "/mcp disable figma extra",
            "/mcp auth figma --project",
            "/mcp add figma https://x y",
        ] {
            assert!(
                parse_mcp_command(bad).unwrap().is_err(),
                "{bad} should be rejected"
            );
        }
    }
}
