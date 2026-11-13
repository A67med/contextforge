//! Integration status model shared by all providers.

use serde::{Deserialize, Serialize};

use crate::Provider;
use crate::detect::AgentInstall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    NotInstalled,
    Installed,
    /// Some but not all ContextForge entries are present.
    Partial,
    /// Entries exist but point to a different ContextForge executable (e.g. after reinstalling elsewhere).
    Outdated,
    /// Installed, but the provider still requires a user action (e.g. Codex hook trust review).
    PendingUserAction,
    /// The configuration file cannot be read or parsed.
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentStatus {
    pub state: ComponentState,
    pub detail: String,
    pub path: Option<String>,
}

impl ComponentStatus {
    pub fn new(state: ComponentState, detail: impl Into<String>, path: Option<&std::path::Path>) -> Self {
        ComponentStatus { state, detail: detail.into(), path: path.map(|p| p.to_string_lossy().into_owned()) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderStatus {
    pub provider: Provider,
    pub label: String,
    pub installations: Vec<AgentInstall>,
    pub mcp: ComponentStatus,
    pub hooks: ComponentStatus,
    /// Codex only: whether the user has reviewed/trusted the hooks.
    pub trust: Option<ComponentStatus>,
    pub notes: Vec<String>,
}

impl ProviderStatus {
    /// Overall state: installed only when both MCP and hooks are installed.
    pub fn overall(&self) -> ComponentState {
        use ComponentState::*;
        match (self.mcp.state, self.hooks.state) {
            (Error, _) | (_, Error) => Error,
            (NotInstalled, NotInstalled) => NotInstalled,
            (Outdated, _) | (_, Outdated) => Outdated,
            (Installed, Installed) => match &self.trust {
                Some(t) if t.state == PendingUserAction => PendingUserAction,
                _ => Installed,
            },
            _ => Partial,
        }
    }
}
