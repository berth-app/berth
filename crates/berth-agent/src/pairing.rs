use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use futures::StreamExt;
use rand::Rng;
use tokio::sync::Mutex;

use berth_proto::nats_relay::{
    generate_pairing_code, pairing_ack_subject, pairing_claim_subject,
    verify_challenge_response, PairingAck, PairingAdvertisement, PairingClaim,
    PAIRING_EXPIRY_SECS,
};

use crate::agent_store::AgentStore;

/// Maximum failed claim attempts before pausing pairing.
const MAX_FAILED_CLAIMS: u32 = 5;

/// Cooldown after too many failed claims (seconds).
const RATE_LIMIT_COOLDOWN_SECS: u64 = 300;

/// Maximum consecutive expired codes before requiring manual restart.
const MAX_EXPIRED_CODES: u32 = 3;

pub struct PairingService {
    client: async_nats::Client,
    agent_id: String,
    store: Arc<Mutex<AgentStore>>,
    env_file: PathBuf,
}

pub struct PairingResult {
    pub owner_id: String,
    pub shared_secret: String,
}

impl PairingService {
    pub fn new(
        client: async_nats::Client,
        agent_id: String,
        store: Arc<Mutex<AgentStore>>,
        env_file: PathBuf,
    ) -> Self {
        Self {
            client,
            agent_id,
            store,
            env_file,
        }
    }

    /// Run the pairing loop. Blocks until an owner claims this agent or timeout.
    /// Returns the owner_id and shared_secret on success, None on expiry.
    pub async fn run(&self) -> Option<PairingResult> {
        self.run_with_counters(0, 0).await
    }

    async fn run_with_counters(
        &self,
        mut expired_count: u32,
        mut failed_claims: u32,
    ) -> Option<PairingResult> {
        let code = generate_pairing_code();

        // Generate a random challenge for this pairing session
        let challenge = uuid::Uuid::new_v4().to_string();

        tracing::info!("[PAIRING] Code: {code} -- Enter this in Berth desktop to pair.");
        println!("[PAIRING] Code: {code} -- Enter this in Berth desktop to pair.");

        let claim_subject = pairing_claim_subject(&code);
        let mut claim_sub = match self.client.subscribe(claim_subject.clone()).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to subscribe to pairing claim subject: {e}");
                return None;
            }
        };

        let advertisement = PairingAdvertisement {
            agent_id: self.agent_id.clone(),
            code: code.clone(),
            hostname: sysinfo::System::host_name().unwrap_or_else(|| "unknown".into()),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            timestamp: Utc::now(),
            challenge: challenge.clone(),
        };

        let deadline = tokio::time::Instant::now()
            + tokio::time::Duration::from_secs(PAIRING_EXPIRY_SECS);

        // Publish initial advertisement immediately
        if let Ok(payload) = serde_json::to_vec(&advertisement) {
            let adv_subject = format!("berth.pairing.advertise.{code}");
            let _ = self.client.publish(adv_subject, payload.into()).await;
        }

        loop {
            // Check rate limiting
            if failed_claims >= MAX_FAILED_CLAIMS {
                tracing::warn!(
                    "[PAIRING] Too many failed claims ({}). Pausing for {}s.",
                    failed_claims,
                    RATE_LIMIT_COOLDOWN_SECS
                );
                println!(
                    "[PAIRING] Too many failed attempts. Pausing for {}s.",
                    RATE_LIMIT_COOLDOWN_SECS
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(RATE_LIMIT_COOLDOWN_SECS)).await;
                // Generate new code after cooldown, reset failed counter
                return Box::pin(self.run_with_counters(expired_count, 0)).await;
            }

            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    expired_count += 1;
                    if expired_count >= MAX_EXPIRED_CODES {
                        tracing::warn!(
                            "[PAIRING] {} consecutive codes expired without pairing. Restart agent to try again.",
                            expired_count
                        );
                        println!(
                            "[PAIRING] {} codes expired. Restart agent to re-enter pairing mode.",
                            expired_count
                        );
                        return None;
                    }
                    tracing::info!("[PAIRING] Code {code} expired. Generating new code...");
                    return Box::pin(self.run_with_counters(expired_count, failed_claims)).await;
                }
                msg = claim_sub.next() => {
                    if let Some(msg) = msg {
                        match serde_json::from_slice::<PairingClaim>(&msg.payload) {
                            Ok(claim) => {
                                // Verify challenge-response if provided
                                if !claim.challenge_response.is_empty() {
                                    if !verify_challenge_response(&challenge, &claim.challenge_response, &code) {
                                        failed_claims += 1;
                                        tracing::warn!(
                                            "[PAIRING] Invalid challenge response (attempt {}/{})",
                                            failed_claims, MAX_FAILED_CLAIMS
                                        );
                                        continue;
                                    }
                                }
                                // Reset counters on successful claim
                                return self.handle_claim(&code, claim).await;
                            }
                            Err(e) => {
                                tracing::warn!("Invalid pairing claim payload: {e}");
                            }
                        }
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(10)) => {
                    // Publish advertisement with fresh timestamp
                    let ad = PairingAdvertisement {
                        timestamp: Utc::now(),
                        ..advertisement.clone()
                    };
                    let adv_subject = format!("berth.pairing.advertise.{code}");
                    if let Ok(payload) = serde_json::to_vec(&ad) {
                        if let Err(e) = self.client.publish(adv_subject, payload.into()).await {
                            tracing::warn!("Failed to publish pairing advertisement: {e}");
                        }
                    }
                }
            }
        }
    }

    async fn handle_claim(&self, code: &str, claim: PairingClaim) -> Option<PairingResult> {
        let owner_id = &claim.owner_id;
        tracing::info!("[PAIRING] Received claim from owner {owner_id}");

        // Generate shared secret for HMAC command signing
        let secret_bytes: [u8; 32] = rand::thread_rng().gen();
        let shared_secret = hex::encode(secret_bytes);

        // Store in SQLite
        {
            let store = self.store.lock().await;
            if let Err(e) = store.set_config("owner_id", owner_id) {
                tracing::error!("Failed to store owner_id: {e}");
                self.send_ack(code, false, "Failed to store owner_id", "")
                    .await;
                return None;
            }
            let _ = store.set_config("paired_at", &Utc::now().to_rfc3339());
            if let Err(e) = store.set_config("shared_secret", &shared_secret) {
                tracing::error!("Failed to store shared_secret: {e}");
                self.send_ack(code, false, "Failed to store shared secret", "")
                    .await;
                return None;
            }
        }

        // Write to agent.env for systemd restart persistence
        self.write_owner_to_env(owner_id);

        // Send ack with shared secret
        self.send_ack(code, true, "Paired successfully", &shared_secret)
            .await;

        tracing::info!(
            "[PAIRING] Paired with owner {owner_id}. HMAC signing enabled."
        );

        Some(PairingResult {
            owner_id: owner_id.clone(),
            shared_secret,
        })
    }

    async fn send_ack(&self, code: &str, success: bool, message: &str, shared_secret: &str) {
        let ack = PairingAck {
            success,
            agent_id: self.agent_id.clone(),
            owner_id: String::new(),
            message: message.into(),
            shared_secret: shared_secret.into(),
        };
        let subject = pairing_ack_subject(code);
        if let Ok(payload) = serde_json::to_vec(&ack) {
            if let Err(e) = self.client.publish(subject, payload.into()).await {
                tracing::warn!("Failed to publish pairing ack: {e}");
            }
        }
    }

    fn write_owner_to_env(&self, owner_id: &str) {
        let env_path = &self.env_file;
        if !env_path.exists() {
            tracing::warn!(
                "agent.env not found at {}, skipping env write",
                env_path.display()
            );
            return;
        }

        match std::fs::read_to_string(env_path) {
            Ok(content) => {
                let new_line = format!("BERTH_OWNER_ID={owner_id}");
                let new_content = if content.contains("BERTH_OWNER_ID=") {
                    content
                        .lines()
                        .map(|line| {
                            if line.starts_with("BERTH_OWNER_ID=") {
                                new_line.as_str()
                            } else {
                                line
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    format!("{content}\n{new_line}")
                };
                if let Err(e) = std::fs::write(env_path, &new_content) {
                    tracing::warn!(
                        "Failed to write BERTH_OWNER_ID to {}: {e}",
                        env_path.display()
                    );
                    return;
                }
                // Restrict file permissions to owner-only
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        env_path,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read {}: {e}", env_path.display());
            }
        }
    }
}
