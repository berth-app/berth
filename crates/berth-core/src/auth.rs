use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// User tier in the Berth licensing model.
/// Anonymous = no account, Free = signed up but no plan.
/// EarlyAdopter = signed up during early access, gets Pro features free forever.
/// Pro and Team are paid tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserTier {
    Anonymous,
    Free,
    EarlyAdopter,
    Pro,
    Team,
}

impl UserTier {
    pub fn can_publish(&self) -> bool {
        matches!(self, Self::EarlyAdopter | Self::Pro | Self::Team)
    }

    pub fn can_use_cloud_targets(&self) -> bool {
        matches!(self, Self::EarlyAdopter | Self::Pro | Self::Team)
    }

    pub fn can_sync_settings(&self) -> bool {
        !matches!(self, Self::Anonymous)
    }

    pub fn can_manage_team(&self) -> bool {
        matches!(self, Self::Team)
    }

    pub fn max_nats_agents(&self) -> Option<u32> {
        match self {
            Self::Anonymous => Some(0),
            Self::Free => Some(3),
            Self::EarlyAdopter | Self::Pro => Some(10),
            Self::Team => None, // unlimited
        }
    }
}

impl std::fmt::Display for UserTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anonymous => write!(f, "anonymous"),
            Self::Free => write!(f, "free"),
            Self::EarlyAdopter => write!(f, "early_adopter"),
            Self::Pro => write!(f, "pro"),
            Self::Team => write!(f, "team"),
        }
    }
}

/// Cached auth state persisted in SQLite settings as JSON.
/// Tokens are NOT stored here — they live in macOS Keychain only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub tier: UserTier,
    pub email: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub last_validated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub offline_grace_until: Option<DateTime<Utc>>,
}

impl AuthState {
    /// Returns the effective tier, accounting for offline grace period expiry.
    /// Pro/Team degrade to Free after 7 days without server validation.
    pub fn effective_tier(&self) -> UserTier {
        match self.tier {
            UserTier::Pro | UserTier::Team => {
                if let Some(grace) = self.offline_grace_until {
                    if Utc::now() > grace {
                        return UserTier::Free;
                    }
                }
                self.tier
            }
            other => other,
        }
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            tier: UserTier::Anonymous,
            email: None,
            user_id: None,
            last_validated_at: None,
            offline_grace_until: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anonymous_capabilities() {
        let tier = UserTier::Anonymous;
        assert!(!tier.can_publish());
        assert!(!tier.can_sync_settings());
        assert_eq!(tier.max_nats_agents(), Some(0));
    }

    #[test]
    fn test_free_capabilities() {
        let tier = UserTier::Free;
        assert!(!tier.can_publish());
        assert!(tier.can_sync_settings());
        assert_eq!(tier.max_nats_agents(), Some(3));
    }

    #[test]
    fn test_early_adopter_capabilities() {
        let tier = UserTier::EarlyAdopter;
        assert!(tier.can_publish());
        assert!(tier.can_use_cloud_targets());
        assert!(tier.can_sync_settings());
        assert!(!tier.can_manage_team());
        assert_eq!(tier.max_nats_agents(), Some(10));
    }

    #[test]
    fn test_pro_capabilities() {
        let tier = UserTier::Pro;
        assert!(tier.can_publish());
        assert!(tier.can_sync_settings());
        assert_eq!(tier.max_nats_agents(), Some(10));
    }

    #[test]
    fn test_team_capabilities() {
        let tier = UserTier::Team;
        assert!(tier.can_manage_team());
        assert_eq!(tier.max_nats_agents(), None);
    }

    #[test]
    fn test_auth_state_default() {
        let state = AuthState::default();
        assert_eq!(state.tier, UserTier::Anonymous);
        assert!(state.email.is_none());
        assert!(state.user_id.is_none());
    }

    #[test]
    fn test_effective_tier_free_no_grace() {
        let state = AuthState {
            tier: UserTier::Free,
            ..Default::default()
        };
        assert_eq!(state.effective_tier(), UserTier::Free);
    }

    #[test]
    fn test_effective_tier_pro_within_grace() {
        let state = AuthState {
            tier: UserTier::Pro,
            offline_grace_until: Some(Utc::now() + chrono::Duration::days(3)),
            ..Default::default()
        };
        assert_eq!(state.effective_tier(), UserTier::Pro);
    }

    #[test]
    fn test_effective_tier_pro_grace_expired() {
        let state = AuthState {
            tier: UserTier::Pro,
            offline_grace_until: Some(Utc::now() - chrono::Duration::days(1)),
            ..Default::default()
        };
        assert_eq!(state.effective_tier(), UserTier::Free);
    }

    #[test]
    fn test_serde_roundtrip() {
        let state = AuthState {
            tier: UserTier::Free,
            email: Some("test@example.com".into()),
            user_id: Some("abc-123".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: AuthState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tier, UserTier::Free);
        assert_eq!(deserialized.email.as_deref(), Some("test@example.com"));
        assert_eq!(deserialized.user_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn test_serde_backwards_compat() {
        // Old AuthState without new fields should deserialize with defaults
        let old_json = r#"{"tier":"free","email":"test@example.com"}"#;
        let state: AuthState = serde_json::from_str(old_json).unwrap();
        assert_eq!(state.tier, UserTier::Free);
        assert!(state.user_id.is_none());
        assert!(state.offline_grace_until.is_none());
    }
}
