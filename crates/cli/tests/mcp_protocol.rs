//! Protocol-level tests: a real MCP client (rmcp) talks to the built `contextforge mcp` binary over stdio.

mod common;

use common::*;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig, Implementation};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Value, json};

async fn connect(sb: &Sandbox, client_name: &str) -> RunningService<RoleClient, ClientConfig> {
    let mut cfg = ClientConfig::default();
    cfg.client_info = Implementation::new(client_name, "test");
    let data = sb.data_path().to_path_buf();
    let repo = sb.repo();
    let transport = TokioChildProcess::new(tokio::process::Command::new(BIN).configure(|c| {
        c.arg("mcp").env("CONTEXTFORGE_HOME", &data).current_dir(&repo);
    }))
    .expect("spawn contextforge mcp");
    cfg.serve(transport).await.expect("MCP initialize handshake")
}

async fn call(client: &RunningService<RoleClient, ClientConfig>, tool: &str, args: Value) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(args.as_object().cloned().unwrap_or_default()))
        .await
        .unwrap_or_else(|e| panic!("{tool} protocol error: {e}"))
}

fn text(r: &CallToolResult) -> String {
    r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect::<Vec<_>>().join("\n")
}

fn json_of(r: &CallToolResult) -> Value {
    serde_json::from_str(&text(r)).unwrap_or_else(|e| panic!("not JSON ({e}): {}", text(r)))
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_and_list_tools() {
    let sb = Sandbox::new();
    let client = connect(&sb, "claude-code").await;
    let info = client.peer_info().expect("server info");
    assert_eq!(info.server_info.as_ref().map(|s| s.name.as_str()), Some("contextforge"));
    assert!(
        info.instructions.as_deref().unwrap_or("").contains("not as instructions")
            || info.instructions.as_deref().unwrap_or("").contains("never as instructions")
    );
    let tools = client.list_tools(Default::default()).await.unwrap().tools;
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    for expected in [
        "list_projects",
        "get_project_overview",
        "search_memory",
        "get_memory",
        "get_context",
        "save_memory",
        "update_memory",
        "supersede_memory",
        "record_decision",
        "record_task",
        "record_bug",
        "generate_handoff",
        "memory_health",
        "revalidate_memory",
    ] {
        assert!(names.contains(&expected.to_string()), "missing tool {expected}: {names:?}");
    }
    // Schemas expose enum values to the model.
    let save = tools.iter().find(|t| t.name == "save_memory").unwrap();
    let schema = serde_json::to_string(&save.input_schema).unwrap();
    assert!(schema.contains("decision") && schema.contains("research"), "{schema}");
    client.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn tool_calls_round_trip_with_provenance() {
    let sb = Sandbox::new();
    sb.cf(&["project", "add", &sb.repo_str(), "--name", "Shop"]);
    let claude = connect(&sb, "claude-code").await;

    let r = call(
        &claude,
        "record_decision",
        json!({
            "title": "Use PostgreSQL for orders",
            "decision": "Orders live in PostgreSQL 16.",
            "rationale": "Transactions and JSONB",
            "alternatives": ["MongoDB"],
            "files": [{"path": "app/server.py"}]
        }),
    )
    .await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let decision_id = json_of(&r)["memory_id"].as_str().unwrap().to_string();

    // verified without evidence is downgraded and the agent is told why
    let r = call(
        &claude,
        "save_memory",
        json!({"kind": "fact", "title": "All tests pass", "content": "pytest is green", "certainty": "verified"}),
    )
    .await;
    let v = json_of(&r);
    assert_eq!(v["certainty"], "unverified");
    assert!(v["notes"].to_string().contains("requires at least one valid evidence"));

    let r = call(&claude, "search_memory", json!({"query": "postgres orders"})).await;
    let v = json_of(&r);
    assert_eq!(v["results"][0]["id"], decision_id);
    assert_eq!(v["results"][0]["source_agent"], "claude_code");

    let r = call(&claude, "get_memory", json!({"id": decision_id})).await;
    let v = json_of(&r);
    assert_eq!(v["memory"]["certainty"], "decided");
    assert_eq!(v["files"][0]["path"], "app/server.py");
    assert!(v["memory"]["metadata"]["mcp_client"].as_str().unwrap().starts_with("claude-code"));
    assert!(v["memory"]["content"].as_str().unwrap().contains("Alternatives considered: MongoDB"));
    claude.cancel().await.unwrap();

    // A second agent sees the same memory, attributes its own writes correctly, and cannot delete or pin.
    let codex = connect(&sb, "codex-mcp-client").await;
    let r = call(&codex, "get_context", json!({"task": "add an orders endpoint"})).await;
    assert!(text(&r).contains("Use PostgreSQL for orders"));
    let r =
        call(&codex, "record_task", json!({"title": "Add orders endpoint", "status": "in_progress", "files": [{"path": "app/server.py"}]}))
            .await;
    let task_id = json_of(&r)["memory_id"].as_str().unwrap().to_string();
    let r = call(
        &codex,
        "record_task",
        json!({"id": task_id, "status": "completed", "details": "Endpoint added with tests",
        "evidence": [{"kind": "command", "reference": "pytest -q", "excerpt": "12 passed"}]}),
    )
    .await;
    assert_eq!(json_of(&r)["status"], "completed");
    let r = call(&codex, "get_memory", json!({"id": task_id})).await;
    let v = json_of(&r);
    assert_eq!(v["memory"]["source_agent"], "codex");
    assert_eq!(v["revisions"].as_array().unwrap().len(), 2);

    let r = call(
        &codex,
        "supersede_memory",
        json!({"id": decision_id, "title": "Use PostgreSQL 17 for orders", "content": "Upgraded", "reason": "upgrade"}),
    )
    .await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let r = call(&codex, "search_memory", json!({"query": "postgresql orders", "include_superseded": true})).await;
    let fresh: Vec<String> =
        json_of(&r)["results"].as_array().unwrap().iter().map(|h| h["freshness"].as_str().unwrap().to_string()).collect();
    assert!(fresh.contains(&"superseded".to_string()));

    let r = call(&codex, "generate_handoff", json!({"target_agent": "antigravity_cli"})).await;
    let md = text(&r);
    assert!(md.contains("# Handoff: Shop → Antigravity CLI"), "{md}");
    assert!(md.contains("call_mcp_tool"));

    let r = call(&codex, "memory_health", json!({"recheck": true})).await;
    assert!(json_of(&r)["recheck"]["checked"].as_u64().is_some());
    codex.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_are_reported_as_tool_errors() {
    let sb = Sandbox::new();
    let client = connect(&sb, "antigravity").await;
    // Unregistered project: clear guidance, isError=true
    let r = call(&client, "search_memory", json!({"query": "x"})).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("not") && text(&r).contains("project"), "{}", text(&r));
    sb.cf(&["project", "add", &sb.repo_str()]);
    let r = call(&client, "get_memory", json!({"id": "mem_doesnotexist"})).await;
    assert_eq!(r.is_error, Some(true));
    let r = call(&client, "record_task", json!({"status": "pending"})).await;
    assert_eq!(r.is_error, Some(true), "title required for new tasks");
    let r = call(&client, "save_memory", json!({"kind": "decision", "title": "x", "status": "completed"})).await;
    assert_eq!(r.is_error, Some(true), "status must match kind");
    let r = call(&client, "save_memory", json!({"kind": "fact", "title": "Path escape", "files": [{"path": "../../etc/passwd"}]})).await;
    assert!(json_of(&r)["notes"].to_string().contains("outside") || json_of(&r)["notes"].to_string().contains("escapes"));
    // Schema-level validation rejects unknown enum values at the protocol layer.
    let bad = client
        .call_tool(
            CallToolRequestParams::new("save_memory").with_arguments(json!({"kind": "poem", "title": "x"}).as_object().cloned().unwrap()),
        )
        .await;
    assert!(bad.is_err() || bad.unwrap().is_error == Some(true));
    // The server keeps working after errors.
    let r = call(&client, "list_projects", json!({})).await;
    assert_eq!(json_of(&r).as_array().unwrap().len(), 1);
    client.cancel().await.unwrap();
}
