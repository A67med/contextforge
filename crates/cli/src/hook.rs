//! `contextforge hook <provider> <event>` — the lifecycle hook entry point used by agents.
//!
//! Contract: read one JSON payload from stdin, print the provider-specific output on stdout, exit 0.
//! Any failure (malformed payload, locked database, internal error, even a panic) produces the provider's
//! documented no-op output, so ContextForge can never block or derail an agent session.

use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use contextforge_core::{OpenOptions, Store};
use contextforge_integrations::{Provider, adapters};

const MAX_STDIN: usize = 8 * 1024 * 1024;
const STDIN_TIMEOUT: Duration = Duration::from_secs(10);

fn read_stdin() -> Vec<u8> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut stdin = std::io::stdin().lock();
        let mut chunk = [0u8; 64 * 1024];
        let mut overflow = false;
        // Keep at most MAX_STDIN bytes but drain the rest, so the agent writing the payload never sees a
        // broken pipe; an oversized payload is then rejected as malformed (no-op output).
        loop {
            match stdin.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() + n <= MAX_STDIN {
                        buf.extend_from_slice(&chunk[..n]);
                    } else {
                        overflow = true;
                    }
                }
            }
        }
        let _ = tx.send(if overflow { Vec::new() } else { buf });
    });
    rx.recv_timeout(STDIN_TIMEOUT).unwrap_or_default()
}

fn process(provider: Provider, event: &str, input: &[u8]) -> Result<(String, Option<String>), String> {
    let text = std::str::from_utf8(input).map_err(|_| "payload is not UTF-8".to_string())?;
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err("empty payload".into());
    }
    let payload: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("malformed JSON payload: {e}"))?;
    let ev = adapters::parse(provider, event, &payload).map_err(|e| e.to_string())?;
    let mut store = Store::open_default(OpenOptions::hook()).map_err(|e| format!("cannot open database: {e}"))?;
    let outcome = store.process_hook_event(&ev).map_err(|e| format!("processing failed: {e}"))?;
    Ok((adapters::render(provider, event, &outcome), outcome.skipped))
}

pub fn run(provider: Provider, event: &str) -> ExitCode {
    let started = Instant::now();
    let input = read_stdin();
    let result = std::panic::catch_unwind(|| process(provider, event, &input)).unwrap_or_else(|_| Err("internal panic".into()));
    let output = match &result {
        Ok((out, _)) => out.clone(),
        Err(_) => adapters::noop(provider, event),
    };
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
    let elapsed = started.elapsed().as_millis() as u64;
    match &result {
        Ok((out, skipped)) => tracing::info!(
            provider = provider.as_str(),
            event,
            elapsed_ms = elapsed,
            injected_chars = out.len(),
            skipped = skipped.as_deref().unwrap_or(""),
            "hook processed"
        ),
        Err(e) => {
            tracing::warn!(provider = provider.as_str(), event, elapsed_ms = elapsed, error = %e, "hook failed; returned no-op output")
        }
    }
    ExitCode::SUCCESS
}
