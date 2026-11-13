//! Live diagnostics: talk MCP to the real ContextForge executable and run its hook command the way an agent
//! would. These prove the installed binary works; provider-side behaviour is verified by live agent tests.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Provider;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub duration_ms: u64,
}

fn spawn(exe: &Path, args: &[&str], data_home: Option<&Path>) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(h) = data_home {
        cmd.env(contextforge_core::db::HOME_ENV, h);
    }
    contextforge_core::process::hide_window(&mut cmd);
    cmd.spawn()
}

/// Perform an MCP `initialize` + `tools/list` handshake over stdio.
pub fn check_mcp(exe: &Path, data_home: Option<&Path>) -> CheckResult {
    let start = Instant::now();
    let name = "MCP server handshake".to_string();
    let result = (|| -> Result<String, String> {
        let mut child = spawn(exe, &["mcp"], data_home).map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let send = |stdin: &mut std::process::ChildStdin, v: Value| -> Result<(), String> {
            writeln!(stdin, "{v}").and_then(|_| stdin.flush()).map_err(|e| format!("write failed: {e}"))
        };
        let wait_for = |id: i64| -> Result<Value, String> {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                let line = rx.recv_timeout(left).map_err(|_| format!("no response to request {id} within 15s"))?;
                let v: Value = serde_json::from_str(&line)
                    .map_err(|e| format!("server wrote non-JSON to stdout ({e}): {}", line.chars().take(120).collect::<String>()))?;
                if v.get("id").and_then(Value::as_i64) == Some(id) {
                    return Ok(v);
                }
            }
        };
        send(
            &mut stdin,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"contextforge-doctor","version":contextforge_core::VERSION}}}),
        )?;
        let init = wait_for(1)?;
        if let Some(err) = init.get("error") {
            return Err(format!("initialize failed: {err}"));
        }
        let server = init["result"]["serverInfo"]["name"].as_str().unwrap_or("?").to_string();
        let protocol = init["result"]["protocolVersion"].as_str().unwrap_or("?").to_string();
        send(&mut stdin, json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        send(&mut stdin, json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))?;
        let tools = wait_for(2)?;
        let names: Vec<String> = tools["result"]["tools"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t["name"].as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        if names.is_empty() {
            return Err("server returned no tools".into());
        }
        Ok(format!("{server} (protocol {protocol}) exposes {} tools: {}", names.len(), names.join(", ")))
    })();
    let duration_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(detail) => CheckResult { name, ok: true, detail, duration_ms },
        Err(detail) => CheckResult { name, ok: false, detail, duration_ms },
    }
}

/// Run `contextforge hook <provider> session-start` with a synthetic payload for an unregistered folder,
/// exactly as the agent would, and check exit status, output format and latency.
pub fn check_hook(exe: &Path, provider: Provider, data_home: Option<&Path>) -> CheckResult {
    let start = Instant::now();
    let name = format!("{} hook command", provider.label());
    let probe_dir = std::env::temp_dir().join("contextforge-hook-probe");
    let _ = std::fs::create_dir_all(&probe_dir);
    let dir = probe_dir.to_string_lossy().into_owned();
    let payload = match provider {
        Provider::Claude => {
            json!({"session_id":"doctor","transcript_path":"","cwd":dir,"hook_event_name":"SessionStart","source":"startup"})
        }
        Provider::Codex => {
            json!({"session_id":"doctor","transcript_path":null,"cwd":dir,"hook_event_name":"SessionStart","model":"doctor","source":"startup"})
        }
        Provider::Antigravity => {
            json!({"conversationId":"doctor","workspacePaths":[dir],"transcriptPath":"","artifactDirectoryPath":"","modelName":"doctor"})
        }
    };
    let result = (|| -> Result<String, String> {
        let mut child = spawn(exe, &["hook", provider.as_str(), "session-start"], data_home)
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        stdin.write_all(payload.to_string().as_bytes()).map_err(|e| e.to_string())?;
        drop(stdin);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                let mut out = String::new();
                if let Some(mut so) = child.stdout.take() {
                    let _ = std::io::Read::read_to_string(&mut so, &mut out);
                }
                if !status.success() {
                    return Err(format!("hook exited with {status}"));
                }
                let out = out.trim();
                if !out.is_empty() {
                    serde_json::from_str::<Value>(out).map_err(|e| format!("hook printed invalid JSON ({e}): {out}"))?;
                }
                return Ok(format!("exit 0, output {}", if out.is_empty() { "empty (no-op)".to_string() } else { out.to_string() }));
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                return Err("hook did not finish within 10s".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    let duration_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(detail) => CheckResult { name, ok: true, detail: format!("{detail} in {duration_ms} ms"), duration_ms },
        Err(detail) => CheckResult { name, ok: false, detail, duration_ms },
    }
}
