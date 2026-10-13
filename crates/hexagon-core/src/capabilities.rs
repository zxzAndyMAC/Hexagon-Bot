//! One read-only capability fact shape for the host recovery surface.
//!
//! Health describes whether an adapter can be reached. `granted` remains a
//! separate approval fact; a healthy adapter never authorizes a side effect.

use crate::session_host::CapabilityHealth;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum CapabilityKind {
    Browser,
    ComputerUse,
    Provider,
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct CapabilityStatus {
    pub kind: CapabilityKind,
    pub id: String,
    pub available: bool,
    pub granted: bool,
    pub health: CapabilityHealth,
    pub last_error: Option<String>,
    pub requires_user_action: bool,
    pub binding_hint: String,
}

fn granted(evidence: Option<bool>) -> bool {
    // The recovery read lane has no agent/turn context. Unknown approval is
    // false on the wire; health is still reported from its own probe.
    evidence == Some(true)
}

#[allow(clippy::too_many_arguments)]
fn status(
    kind: CapabilityKind,
    id: impl Into<String>,
    available: bool,
    granted: bool,
    health: CapabilityHealth,
    last_error: Option<&str>,
    requires_user_action: bool,
    binding_hint: &str,
) -> CapabilityStatus {
    CapabilityStatus {
        kind,
        id: id.into(),
        available,
        granted,
        health,
        last_error: last_error.map(str::to_owned),
        requires_user_action,
        binding_hint: binding_hint.to_owned(),
    }
}

pub fn browser(
    id: &str,
    connected: Option<bool>,
    grant: Option<bool>,
    binding_hint: &str,
) -> CapabilityStatus {
    match connected {
        Some(true) => status(
            CapabilityKind::Browser,
            id,
            true,
            granted(grant),
            CapabilityHealth::Healthy,
            None,
            grant != Some(true),
            binding_hint,
        ),
        Some(false) => status(
            CapabilityKind::Browser,
            id,
            false,
            granted(grant),
            CapabilityHealth::Unavailable,
            Some("browser_disconnected"),
            true,
            binding_hint,
        ),
        None => status(
            CapabilityKind::Browser,
            id,
            false,
            granted(grant),
            CapabilityHealth::Unknown,
            Some("browser_status_unknown"),
            true,
            binding_hint,
        ),
    }
}

pub fn computer_use(
    id: &str,
    component_available: Option<bool>,
    os_granted: Option<bool>,
    project_granted: Option<bool>,
    paused: bool,
    reconnecting: bool,
    binding_hint: &str,
) -> CapabilityStatus {
    let health = if reconnecting {
        CapabilityHealth::Reconnecting
    } else {
        match component_available {
            Some(false) => CapabilityHealth::Unavailable,
            None => CapabilityHealth::Unknown,
            Some(true) if paused || os_granted == Some(false) => CapabilityHealth::Degraded,
            Some(true) if os_granted.is_none() => CapabilityHealth::Unknown,
            Some(true) => CapabilityHealth::Healthy,
        }
    };
    let error = match (component_available, os_granted) {
        (Some(false), _) => Some("computer_component_unavailable"),
        (None, _) => Some("computer_component_status_unknown"),
        (Some(true), Some(false)) => Some("computer_os_permission_missing"),
        (Some(true), None) => Some("computer_os_permission_unknown"),
        (Some(true), Some(true)) => None,
    };
    let requires_user_action = paused
        || component_available != Some(true)
        || os_granted != Some(true)
        || project_granted != Some(true);
    status(
        CapabilityKind::ComputerUse,
        id,
        component_available == Some(true),
        component_available == Some(true) && os_granted == Some(true) && granted(project_granted),
        health,
        error,
        requires_user_action,
        binding_hint,
    )
}

pub fn provider(
    id: &str,
    configured: Option<bool>,
    key_present: Option<bool>,
    grant: Option<bool>,
    binding_hint: &str,
) -> CapabilityStatus {
    let available = configured == Some(true) && key_present == Some(true);
    let known_unavailable = configured == Some(false) || key_present == Some(false);
    status(
        CapabilityKind::Provider,
        id,
        available,
        granted(grant),
        if known_unavailable {
            CapabilityHealth::Unavailable
        } else {
            CapabilityHealth::Unknown
        },
        if known_unavailable {
            Some("provider_not_configured")
        } else if !available {
            Some("provider_status_unknown")
        } else {
            None
        },
        !available,
        binding_hint,
    )
}

pub fn mcp(
    id: &str,
    live: Option<bool>,
    grant: Option<bool>,
    binding_hint: &str,
) -> CapabilityStatus {
    match live {
        Some(true) => status(
            CapabilityKind::Mcp,
            id,
            true,
            granted(grant),
            CapabilityHealth::Healthy,
            None,
            grant != Some(true),
            binding_hint,
        ),
        Some(false) => status(
            CapabilityKind::Mcp,
            id,
            false,
            granted(grant),
            CapabilityHealth::Unavailable,
            Some("mcp_unavailable"),
            true,
            binding_hint,
        ),
        None => status(
            CapabilityKind::Mcp,
            id,
            false,
            granted(grant),
            CapabilityHealth::Unknown,
            Some("mcp_status_unknown"),
            true,
            binding_hint,
        ),
    }
}

/// Project a live MCP host row without turning liveness into permission.
/// `available` and `health` come from the owned process probe; approval stays
/// unknown in the read/beat lane and therefore remains false on the wire.
pub fn mcp_observed(
    id: &str,
    available: bool,
    health: CapabilityHealth,
    last_error: Option<&str>,
    binding_hint: &str,
) -> CapabilityStatus {
    status(
        CapabilityKind::Mcp,
        id,
        available,
        false,
        health,
        last_error,
        health != CapabilityHealth::Healthy,
        binding_hint,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn health_and_grant_are_independent() {
        let status = browser("b", Some(true), Some(false), "project:/tmp/p");
        assert_eq!(status.health, CapabilityHealth::Healthy);
        assert!(!status.granted);
        assert!(status.requires_user_action);
    }

    #[test]
    fn unavailable_facts_request_owner_action() {
        let status = computer_use(
            "computer",
            Some(false),
            Some(false),
            Some(false),
            false,
            false,
            "project:/tmp/p",
        );
        assert_eq!(status.health, CapabilityHealth::Unavailable);
        assert!(status.requires_user_action);
        assert!(!status.granted);
    }

    #[test]
    fn unknown_grant_or_os_probe_does_not_change_health() {
        let browser = browser("b", Some(true), None, "project:/tmp/p");
        assert_eq!(browser.health, CapabilityHealth::Healthy);
        assert!(!browser.granted);

        let computer = computer_use(
            "computer",
            Some(true),
            None,
            Some(true),
            false,
            false,
            "project:/tmp/p",
        );
        assert_eq!(computer.health, CapabilityHealth::Unknown);
        assert!(!computer.granted);

        let provider = provider("p", Some(true), Some(true), None, "project:/tmp/p");
        assert!(provider.available);
        assert_eq!(provider.health, CapabilityHealth::Unknown);
        assert!(!provider.granted);
    }

    #[test]
    fn observed_mcp_health_never_grants_permission() {
        let status = mcp_observed(
            "svc",
            true,
            CapabilityHealth::Healthy,
            None,
            "project:/tmp/p",
        );
        assert!(status.available);
        assert_eq!(status.health, CapabilityHealth::Healthy);
        assert!(!status.granted);
    }

    proptest! {
        #[test]
        fn health_never_implies_grant(connected in any::<bool>(), grant in prop::option::of(any::<bool>())) {
            let status = browser("b", Some(connected), grant, "project:/tmp/p");
            prop_assert_eq!(status.granted, grant == Some(true));
        }

        #[test]
        fn unknown_runtime_or_grant_fails_closed(
            connected in any::<bool>(),
            component in prop::option::of(any::<bool>()),
            os in prop::option::of(any::<bool>()),
            project in prop::option::of(any::<bool>()),
        ) {
            let browser = browser("b", Some(connected), None, "project:/tmp/p");
            prop_assert!(!browser.granted);
            let computer = computer_use("computer", component, os, project, false, false, "project:/tmp/p");
            prop_assert_eq!(computer.granted, component == Some(true) && os == Some(true) && project == Some(true));
        }
    }
}
