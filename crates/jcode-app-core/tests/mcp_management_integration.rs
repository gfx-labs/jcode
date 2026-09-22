//! Real management-tool execution against isolated global and project configs.
//! No LLM, daemon, browser, or user MCP configuration is involved.
use jcode_app_core::{
    mcp::{McpConfig, McpManager, McpSchemaCache, SharedMcpPool},
    tool::{
        Tool, ToolContext, ToolExecutionMode,
        mcp::{McpCallTool, McpManagementTool, McpSearchTool},
    },
};
use serde_json::{Value, json};
use std::{
    process::{Command, Stdio},
    sync::Arc,
};
use tokio::sync::RwLock;

const SERVER: &str = r#"
import json,sys
for line in sys.stdin:
 req=json.loads(line); method=req.get('method')
 if 'id' not in req: continue
 result=({'protocolVersion':'2024-11-05','capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}} if method=='initialize' else
         {'tools':[{'name':'echo','inputSchema':{'type':'object'},'description':'fixture tool'}]} if method=='tools/list' else
         {'content':[{'type':'text','text':json.dumps(req['params']['arguments'])}]} if method=='tools/call' else {})
 print(json.dumps({'jsonrpc':'2.0','id':req['id'],'result':result}),flush=True)
"#;

fn context(project: &std::path::Path) -> ToolContext {
    ToolContext {
        session_id: "scoped-mcp-test".into(),
        message_id: "message".into(),
        tool_call_id: "call".into(),
        working_dir: Some(project.into()),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: ToolExecutionMode::Direct,
    }
}

async fn run(tool: &impl Tool, input: Value, ctx: &ToolContext) -> String {
    tool.execute(input, ctx.clone()).await.unwrap().output
}

#[tokio::test]
async fn management_project_override_changes_search_and_call_without_mutating_global() {
    if !Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
    {
        eprintln!("SKIP: python3 unavailable");
        return;
    }
    let sandbox = tempfile::tempdir().unwrap();
    let project = sandbox.path().join("project");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    let home = sandbox.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    unsafe {
        std::env::set_var("JCODE_HOME", &home);
    }
    let global = home.join("mcp.json");
    std::fs::write(
        &global,
        json!({"servers":{"fixture":{
            "command":"python3", "args":["-I","-S","-u","-c",SERVER],"shared":false
        }}})
        .to_string(),
    )
    .unwrap();
    let pool = Arc::new(SharedMcpPool::new(McpConfig::default()));
    let manager = Arc::new(RwLock::new(McpManager::with_shared_pool_for_dir(
        pool,
        "scoped-mcp-test".into(),
        Some(project.clone()),
    )));
    let management = McpManagementTool::new(manager.clone());
    let search = McpSearchTool::new(manager.clone());
    let call = McpCallTool::new(manager.clone());
    let ctx = context(&project);
    let (connected, failures) = manager.read().await.connect_all().await.unwrap();
    assert_eq!(connected, 1, "{failures:?}");
    let found: Value =
        serde_json::from_str(&run(&search, json!({"server":"fixture"}), &ctx).await).unwrap();
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["tool"], "echo");
    assert!(
        run(
            &call,
            json!({"server":"fixture","tool":"echo","arguments":{"live":true}}),
            &ctx
        )
        .await
        .contains("live")
    );

    // Seed the same schema cache used to advertise tools before connection.
    // Disabling must hide it even when the cache still contains the entry.
    let mut cache = McpSchemaCache::default();
    let cfg = manager.read().await.config().servers["fixture"].clone();
    let defs = manager
        .read()
        .await
        .all_tools()
        .await
        .into_iter()
        .map(|(_, tool)| tool)
        .collect();
    assert!(cache.update("fixture", &cfg, defs));
    cache.save();
    assert_eq!(McpSchemaCache::load().server_count(), 1);
    let disabled = run(
        &management,
        json!({"action":"disable","server":"fixture","scope":"project"}),
        &ctx,
    )
    .await;
    assert!(disabled.contains("project disabled"), "{disabled}");
    let project_config = project.join(".jcode/mcp.json");
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&project_config).unwrap()).unwrap()["servers"]
            ["fixture"]["enabled"],
        false
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&global).unwrap()).unwrap()["servers"]["fixture"]
            ["command"],
        "python3"
    );
    assert!(serde_json::from_slice::<Value>(&std::fs::read(&global).unwrap()).unwrap()["servers"]["fixture"].get("enabled").is_none());
    let hidden: Value =
        serde_json::from_str(&run(&search, json!({"server":"fixture"}), &ctx).await).unwrap();
    assert!(
        hidden.as_array().unwrap().is_empty(),
        "disabled tool leaked in search: {hidden}"
    );
    let error = call
        .execute(
            json!({"server":"fixture","tool":"echo","arguments":{}}),
            ctx.clone(),
        )
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("disabled"), "{error:#}");
    assert_eq!(
        McpSchemaCache::load().server_count(),
        1,
        "cache should remain available for re-enable"
    );
    assert!(
        manager.read().await.searchable_tools().await.is_empty(),
        "cached disabled schema leaked"
    );

    let enabled = run(
        &management,
        json!({"action":"enable","server":"fixture","scope":"project"}),
        &ctx,
    )
    .await;
    assert!(enabled.contains("project enabled"), "{enabled}");
    let found: Value =
        serde_json::from_str(&run(&search, json!({"server":"fixture"}), &ctx).await).unwrap();
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert!(
        run(
            &call,
            json!({"server":"fixture","tool":"echo","arguments":{"restored":true}}),
            &ctx
        )
        .await
        .contains("restored")
    );
    let added = run(
        &management,
        json!({"action":"add","server":"remote","scope":"project","url":"http://127.0.0.1:1/mcp"}),
        &ctx,
    )
    .await;
    assert!(added.contains("Added MCP server 'remote'"), "{added}");
    let project_file: Value =
        serde_json::from_slice(&std::fs::read(project_config).unwrap()).unwrap();
    assert_eq!(
        project_file["servers"]["remote"]["url"],
        "http://127.0.0.1:1/mcp"
    );
    assert!(
        serde_json::from_slice::<Value>(&std::fs::read(global).unwrap()).unwrap()["servers"]
            .get("remote")
            .is_none()
    );
    manager.read().await.disconnect_all().await;
}
