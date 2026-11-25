//! Tweak model: identity, risk, the user-visible state and the plan.
//!
//! A tweak is pure data (`TweakDefinition`). Everything the engine does is
//! derived from its action list, so adding a catalog item never means touching
//! the engine or the UI.

use std::fmt;

use crate::action::Action;
use crate::journal::Snapshot;

/// How dangerous a tweak is. Mirrors the 🟢/🟡/🟠/🔴 marks in `FEATURES.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    /// Cosmetic change, easy to revert.
    Low,
    /// Changes system behaviour.
    Medium,
    /// Services, security, network.
    High,
    /// Data loss or boot problems.
    Critical,
}

impl RiskLevel {
    /// Text used next to the item (the UI localizes it further if needed).
    pub fn label(self) -> &'static str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        }
    }

    /// Whether the UI must show a separate confirmation with the risk text.
    pub fn requires_confirmation(self) -> bool {
        matches!(self, RiskLevel::High | RiskLevel::Critical)
    }
}

/// One catalog item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TweakDefinition {
    /// Stable id, e.g. `PRIV-01`.
    pub id: String,
    /// Section id, e.g. `privacy`.
    pub section: String,
    /// Localization key of the title.
    pub title_key: String,
    /// Localization key of the description.
    pub description_key: String,
    /// Risk level.
    pub risk: RiskLevel,
    /// Whether Explorer or the system must be restarted.
    pub requires_reboot: bool,
    /// Ordered actions that implement the tweak.
    pub actions: Vec<Action>,
}

impl TweakDefinition {
    /// Builds a definition; ids and keys are validated separately.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        section: impl Into<String>,
        title_key: impl Into<String>,
        description_key: impl Into<String>,
        risk: RiskLevel,
        requires_reboot: bool,
        actions: Vec<Action>,
    ) -> Self {
        Self {
            id: id.into(),
            section: section.into(),
            title_key: title_key.into(),
            description_key: description_key.into(),
            risk,
            requires_reboot,
            actions,
        }
    }

    /// `true` for 🔴 items that need a dedicated confirmation.
    pub fn is_dangerous(&self) -> bool {
        self.risk == RiskLevel::Critical
    }

    /// Checks that the definition can be stored in the catalog.
    pub fn validate(&self) -> Result<(), DefinitionError> {
        if self.id.trim().is_empty() {
            return Err(DefinitionError::EmptyId);
        }
        if !valid_id(&self.id) {
            return Err(DefinitionError::BadId(self.id.clone()));
        }
        if self.section.trim().is_empty() {
            return Err(DefinitionError::EmptySection);
        }
        if self.title_key.trim().is_empty() || self.description_key.trim().is_empty() {
            return Err(DefinitionError::EmptyKey(self.id.clone()));
        }
        if self.actions.is_empty() {
            return Err(DefinitionError::NoActions(self.id.clone()));
        }
        Ok(())
    }
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    id.chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

/// Problems found while validating a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionError {
    /// The id is empty.
    EmptyId,
    /// The id does not match `SECTION-NN`.
    BadId(String),
    /// The section is empty.
    EmptySection,
    /// A localization key is empty.
    EmptyKey(String),
    /// The action list is empty.
    NoActions(String),
}

impl fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DefinitionError::EmptyId => f.write_str("tweak id is empty"),
            DefinitionError::BadId(id) => write!(f, "invalid tweak id: {id}"),
            DefinitionError::EmptySection => f.write_str("tweak section is empty"),
            DefinitionError::EmptyKey(id) => write!(f, "empty localization key in {id}"),
            DefinitionError::NoActions(id) => write!(f, "tweak {id} has no actions"),
        }
    }
}

impl std::error::Error for DefinitionError {}

/// What the user sees in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TweakState {
    /// Every action already has its target value.
    Applied,
    /// At least one action still has to be executed.
    NotApplied,
    /// The tweak cannot be applied here, with a reason (missing service, OS...).
    Unavailable {
        /// Human-readable reason, shown in the UI and the report.
        reason: String,
    },
}

impl TweakState {
    /// Short text for the UI.
    pub fn label(&self) -> &'static str {
        match self {
            TweakState::Applied => "applied",
            TweakState::NotApplied => "not applied",
            TweakState::Unavailable { .. } => "unavailable",
        }
    }
}

/// One step of the preview: what will change and what was there before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    /// Action to execute.
    pub action: Action,
    /// Snapshot taken right before planning.
    pub before: Snapshot,
    /// Technical one-line description.
    pub preview: String,
}

/// Everything that will be done for one tweak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Tweak id.
    pub tweak_id: String,
    /// Actions that still have to run, with their "before" snapshots.
    pub steps: Vec<PlanStep>,
    /// Actions that were already in the target state.
    pub skipped: Vec<Action>,
}

impl Plan {
    /// `true` when nothing has to be changed.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Number of actions to execute.
    pub fn len(&self) -> usize {
        self.steps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::RegistryKey;
    use crate::action::RegistryRoot;
    use crate::action::RegistryValue;

    fn registry_action() -> Action {
        Action::RegistrySet {
            key: RegistryKey::new(RegistryRoot::Hkcu, "Software\\Test"),
            name: "Value".into(),
            value: RegistryValue::Dword(1),
        }
    }

    fn definition(id: &str, actions: Vec<Action>) -> TweakDefinition {
        TweakDefinition::new(
            id,
            "privacy",
            "tweak.title",
            "tweak.description",
            RiskLevel::Medium,
            false,
            actions,
        )
    }

    #[test]
    fn valid_definition_passes_validation() {
        assert!(
            definition("PRIV-01", vec![registry_action()])
                .validate()
                .is_ok()
        );
        assert!(
            definition("SYS-AZ", vec![registry_action()])
                .validate()
                .is_ok()
        );
        assert!(
            definition("CLN-START", vec![registry_action()])
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn invalid_ids_are_rejected() {
        for id in ["", "priv-01", "1PRIV", "PRIV 01", "priv"] {
            assert!(
                definition(id, vec![registry_action()]).validate().is_err(),
                "{id} must be rejected"
            );
        }
    }

    #[test]
    fn empty_actions_are_rejected() {
        assert!(matches!(
            definition("PRIV-02", vec![]).validate(),
            Err(DefinitionError::NoActions(_))
        ));
    }

    #[test]
    fn critical_tweaks_need_confirmation() {
        let mut tweak = definition("SYS-04", vec![registry_action()]);
        tweak.risk = RiskLevel::Critical;
        assert!(tweak.is_dangerous());
        assert!(RiskLevel::High.requires_confirmation());
        assert!(!RiskLevel::Medium.requires_confirmation());
    }

    #[test]
    fn plan_empty_flag_matches_steps() {
        let plan = Plan {
            tweak_id: "PRIV-01".into(),
            steps: vec![],
            skipped: vec![registry_action()],
        };
        assert!(plan.is_empty());
        assert_eq!(plan.len(), 0);
    }
}
