//! Provider adapters: parse each provider's documented hook payload into the shared
//! [`HookEvent`](contextforge_core::capture::HookEvent) model and render the provider-specific output.
//!
//! The three providers do **not** share a schema: Claude Code and Codex use snake_case fields and
//! `hookSpecificOutput.additionalContext`; Antigravity uses camelCase fields and `injectSteps`.

pub mod antigravity;
pub mod claude;
pub mod codex;

use contextforge_core::capture::{HookEvent, HookOutcome};
use serde_json::Value;

use crate::Provider;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("unsupported {provider} hook event '{event}'")]
    UnsupportedEvent { provider: &'static str, event: String },
    #[error("invalid hook payload: {0}")]
    InvalidPayload(String),
}

/// Canonicalise an event argument (`session-start`, `session_start`, `SessionStart`) to PascalCase.
pub fn canonical_event(arg: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in arg.trim().chars() {
        if ch == '-' || ch == '_' || ch == ' ' {
            upper = true;
            continue;
        }
        if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Events each adapter understands (PascalCase, as the provider names them).
pub fn supported_events(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => claude::EVENTS,
        Provider::Codex => codex::EVENTS,
        Provider::Antigravity => antigravity::EVENTS,
    }
}

pub fn parse(provider: Provider, event: &str, payload: &Value) -> Result<HookEvent, AdapterError> {
    let event = canonical_event(event);
    if !payload.is_object() {
        return Err(AdapterError::InvalidPayload("payload must be a JSON object".into()));
    }
    match provider {
        Provider::Claude => claude::parse(&event, payload),
        Provider::Codex => codex::parse(&event, payload),
        Provider::Antigravity => antigravity::parse(&event, payload),
    }
}

/// Text to print on stdout for this event (may be empty).
pub fn render(provider: Provider, event: &str, outcome: &HookOutcome) -> String {
    let event = canonical_event(event);
    match provider {
        Provider::Claude => claude::render(&event, outcome.context.as_deref()),
        Provider::Codex => codex::render(&event, outcome.context.as_deref()),
        Provider::Antigravity => antigravity::render(&event, outcome.context.as_deref()),
    }
}

/// Safe "do nothing" output for an event, used whenever ContextForge itself fails.
/// Hooks must never block or derail the agent.
pub fn noop(provider: Provider, event: &str) -> String {
    render(provider, event, &HookOutcome::default())
}

pub(crate) fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

pub(crate) fn string_field(v: &Value, key: &str) -> Option<String> {
    str_field(v, key).map(str::to_string)
}

/// Commands may arrive as a string or as an argv array.
pub(crate) fn command_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Array(parts) => {
            let s: Vec<String> = parts.iter().filter_map(|p| p.as_str().map(str::to_string)).collect();
            if s.is_empty() { None } else { Some(s.join(" ")) }
        }
        _ => None,
    }
}

/// Clamp injected context to a provider's limit on a character boundary.
pub(crate) fn clamp(ctx: &str, max_chars: usize) -> String {
    if ctx.chars().count() <= max_chars {
        return ctx.to_string();
    }
    let mut s: String = ctx.chars().take(max_chars.saturating_sub(40)).collect();
    s.push_str("\n…(truncated)\n</contextforge-memory>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_event_names() {
        assert_eq!(canonical_event("session-start"), "SessionStart");
        assert_eq!(canonical_event("user_prompt_submit"), "UserPromptSubmit");
        assert_eq!(canonical_event("PostToolUse"), "PostToolUse");
        assert_eq!(canonical_event("pre-invocation"), "PreInvocation");
    }

    #[test]
    fn clamp_keeps_short_text() {
        assert_eq!(clamp("abc", 10), "abc");
        assert!(clamp(&"x".repeat(100), 60).chars().count() <= 60 + 30);
    }
}
