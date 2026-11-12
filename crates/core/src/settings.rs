//! Application settings (stored in the database so every process sees the same values).

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, Store, util};

const SETTINGS_KEY: &str = "app";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Settings {
    pub context: ContextSettings,
    pub capture: CaptureSettings,
    pub retention: RetentionSettings,
    pub privacy: PrivacySettings,
    pub freshness: FreshnessSettings,
    pub logging: LoggingSettings,
    pub ui: UiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ContextSettings {
    /// Token budget for the brief injected when an agent session starts.
    pub startup_budget_tokens: usize,
    /// Token budget for prompt-relevant memories injected per user prompt.
    pub prompt_budget_tokens: usize,
    /// Inject prompt-relevant memories (UserPromptSubmit / PreInvocation).
    pub prompt_injection: bool,
    /// Default budget for handoff briefs.
    pub handoff_budget_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CaptureSettings {
    /// Record shell commands run by agents (redacted, truncated).
    pub commands: bool,
    /// Store user prompt text. Off by default: prompts are used transiently for retrieval only.
    pub prompts: bool,
    /// Store a redacted excerpt of the agent's last message per turn as the session summary.
    pub assistant_summaries: bool,
    /// Turn successful build/test/lint commands into verified `command` memories.
    pub auto_command_memories: bool,
    /// Register Git repositories automatically when an agent session starts in them.
    pub auto_register_projects: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RetentionSettings {
    /// Raw activity events older than this are pruned (memories are never pruned automatically).
    pub events_days: u32,
    /// Handoff briefs kept per project.
    pub handoffs_per_project: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PrivacySettings {
    /// Extra glob patterns treated as sensitive (in addition to built-in defaults).
    pub extra_sensitive_globs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FreshnessSettings {
    /// Re-evaluate freshness when an agent session starts.
    pub check_on_session_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LoggingSettings {
    /// trace | debug | info | warn | error
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiSettings {
    /// system | dark | light
    pub theme: String,
}

impl Default for ContextSettings {
    fn default() -> Self {
        ContextSettings { startup_budget_tokens: 1200, prompt_budget_tokens: 300, prompt_injection: true, handoff_budget_tokens: 1500 }
    }
}
impl Default for CaptureSettings {
    fn default() -> Self {
        CaptureSettings {
            commands: true,
            prompts: false,
            assistant_summaries: true,
            auto_command_memories: true,
            auto_register_projects: false,
        }
    }
}
impl Default for RetentionSettings {
    fn default() -> Self {
        RetentionSettings { events_days: 90, handoffs_per_project: 50 }
    }
}
impl Default for FreshnessSettings {
    fn default() -> Self {
        FreshnessSettings { check_on_session_start: true }
    }
}
impl Default for LoggingSettings {
    fn default() -> Self {
        LoggingSettings { level: "info".into() }
    }
}
impl Default for UiSettings {
    fn default() -> Self {
        UiSettings { theme: "system".into() }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        let c = &self.context;
        if !(200..=8000).contains(&c.startup_budget_tokens) {
            return Err(Error::invalid("startup context budget must be between 200 and 8000 tokens"));
        }
        if !(50..=4000).contains(&c.prompt_budget_tokens) {
            return Err(Error::invalid("prompt context budget must be between 50 and 4000 tokens"));
        }
        if !(300..=12000).contains(&c.handoff_budget_tokens) {
            return Err(Error::invalid("handoff budget must be between 300 and 12000 tokens"));
        }
        if !(1..=3650).contains(&self.retention.events_days) {
            return Err(Error::invalid("event retention must be between 1 and 3650 days"));
        }
        if !(1..=1000).contains(&self.retention.handoffs_per_project) {
            return Err(Error::invalid("handoff retention must be between 1 and 1000"));
        }
        if !["trace", "debug", "info", "warn", "error"].contains(&self.logging.level.as_str()) {
            return Err(Error::invalid("log level must be one of trace, debug, info, warn, error"));
        }
        if !["system", "dark", "light"].contains(&self.ui.theme.as_str()) {
            return Err(Error::invalid("theme must be system, dark or light"));
        }
        for g in &self.privacy.extra_sensitive_globs {
            crate::privacy::validate_glob(g)?;
        }
        Ok(())
    }
}

impl Store {
    pub fn settings(&self) -> Result<Settings> {
        let raw: Option<String> =
            self.conn.query_row("SELECT value_json FROM settings WHERE key = ?1", [SETTINGS_KEY], |r| r.get(0)).optional()?;
        match raw {
            // Unknown or malformed values fall back to defaults field-by-field (serde(default)).
            Some(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        settings.validate()?;
        let json = serde_json::to_string(settings)?;
        self.write_tx(|tx| {
            tx.execute(
                "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
                params![SETTINGS_KEY, json, util::now_ts()],
            )?;
            Ok(())
        })
    }

    pub fn sensitive_matcher(&self, project_extra: &[String]) -> Result<crate::privacy::SensitiveMatcher> {
        let mut globs = self.settings()?.privacy.extra_sensitive_globs;
        globs.extend(project_extra.iter().cloned());
        Ok(crate::privacy::SensitiveMatcher::new(&globs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_persist_and_validate() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in(dir.path(), crate::OpenOptions::interactive()).unwrap();
        let mut s = store.settings().unwrap();
        assert_eq!(s, Settings::default());
        s.context.startup_budget_tokens = 900;
        s.ui.theme = "dark".into();
        store.save_settings(&s).unwrap();
        assert_eq!(store.settings().unwrap().context.startup_budget_tokens, 900);
        s.context.startup_budget_tokens = 10;
        assert!(store.save_settings(&s).is_err());
        s.context.startup_budget_tokens = 900;
        s.privacy.extra_sensitive_globs = vec!["[".into()];
        assert!(store.save_settings(&s).is_err());
    }

    #[test]
    fn partial_json_uses_field_defaults() {
        let parsed: Settings = serde_json::from_str(r#"{"context":{"startup_budget_tokens":700}}"#).unwrap();
        assert_eq!(parsed.context.startup_budget_tokens, 700);
        assert_eq!(parsed.context.prompt_budget_tokens, 300);
        assert!(parsed.capture.commands);
    }
}
