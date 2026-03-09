use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

// Public values — not secrets. Safe to ship in binary (like Firebase config).
// The publishable key only permits operations allowed by RLS policies.
const SUPABASE_URL: &str = "https://zwkapibxhxakavnujtzy.supabase.co";
const SUPABASE_PUBLISHABLE_KEY: &str = "sb_publishable_SPFZCc8di3q468V555mlwA_6W1lcBuP";

/// Tokens returned by Supabase after successful authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: Option<u64>,
}

/// User profile from the `profiles` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: String,
    pub email: Option<String>,
    pub tier: String,
}

/// Supabase REST client for auth operations.
pub struct SupabaseClient {
    http: reqwest::Client,
}

impl SupabaseClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("berth-app")
            .build()
            .expect("Failed to build HTTP client");
        Self { http }
    }

    /// Returns true if Supabase is configured (constants are not placeholder values).
    pub fn is_configured() -> bool {
        !SUPABASE_URL.contains("YOUR_PROJECT") && !SUPABASE_PUBLISHABLE_KEY.contains("YOUR_")
    }

    fn ensure_configured() -> Result<()> {
        if !Self::is_configured() {
            bail!("Supabase is not configured. Update SUPABASE_URL and SUPABASE_PUBLISHABLE_KEY in supabase.rs");
        }
        Ok(())
    }

    /// Send a magic link email to the user. Creates account if it doesn't exist.
    pub async fn send_magic_link(&self, email: &str) -> Result<()> {
        Self::ensure_configured()?;

        let resp = self
            .http
            .post(format!("{SUPABASE_URL}/auth/v1/otp"))
            .header("apikey", SUPABASE_PUBLISHABLE_KEY)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "email": email,
                "create_user": true
            }))
            .send()
            .await
            .context("Failed to send magic link request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Magic link failed (HTTP {status}): {body}");
        }

        Ok(())
    }

    /// Exchange a refresh token for new access + refresh tokens.
    pub async fn refresh_session(&self, refresh_token: &str) -> Result<AuthTokens> {
        Self::ensure_configured()?;

        let resp = self
            .http
            .post(format!("{SUPABASE_URL}/auth/v1/token?grant_type=refresh_token"))
            .header("apikey", SUPABASE_PUBLISHABLE_KEY)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "refresh_token": refresh_token
            }))
            .send()
            .await
            .context("Failed to refresh session")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Token refresh failed (HTTP {status}): {body}");
        }

        resp.json::<AuthTokens>()
            .await
            .context("Failed to parse refresh response")
    }

    /// Fetch the user's profile from the `profiles` table.
    pub async fn get_profile(&self, access_token: &str) -> Result<UserProfile> {
        Self::ensure_configured()?;

        let resp = self
            .http
            .get(format!("{SUPABASE_URL}/rest/v1/profiles?select=id,email,tier"))
            .header("apikey", SUPABASE_PUBLISHABLE_KEY)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .context("Failed to fetch profile")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Profile fetch failed (HTTP {status}): {body}");
        }

        let profiles: Vec<UserProfile> = resp
            .json()
            .await
            .context("Failed to parse profile response")?;

        profiles
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No profile found for this user"))
    }

    /// Sign out: revoke the user's session on the server.
    pub async fn sign_out(&self, access_token: &str) -> Result<()> {
        Self::ensure_configured()?;

        let _ = self
            .http
            .post(format!("{SUPABASE_URL}/auth/v1/logout"))
            .header("apikey", SUPABASE_PUBLISHABLE_KEY)
            .header("Authorization", format!("Bearer {access_token}"))
            .send()
            .await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_configured() {
        assert!(SupabaseClient::is_configured());
    }
}
