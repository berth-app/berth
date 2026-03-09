use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use futures::StreamExt;
use tokio::sync::Mutex;

use berth_proto::nats_relay::{
    generate_pairing_code, pairing_ack_subject, pairing_advertise_subject, pairing_claim_subject,
    PairingAck, PairingAdvertisement, PairingClaim, PAIRING_EXPIRY_SECS,
};

use crate::agent_store::AgentStore;

pub struct PairingService {
    client: async_nats::Client,
    agent_id: String,
    store: Arc<Mutex<AgentStore>>,
    env_file: PathBuf,
}

pub struct PairingResult {
    pub owner_id: String,
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
    /// Returns the owner_id on success, None on expiry.
    pub async fn run(&self) -> Option<PairingResult> {
        let code = generate_pairing_code();

        tracing::info!("[PAIRING] Code: {code} -- Enter this in Berth desktop to pair.");
        // Also print to stdout for systemd journal visibility
        println!("[PAIRING] Code: {code} -- Enter this in Berth desktop to pair.");

        let claim_subject = pairing_claim_subject(&code);
        let mut claim_sub = match self.client.subscribe(claim_subject.clone()).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to subscribe to pairing claim subject: {e}");
                return None;
            }
        };

        let advertise_subject = pairing_advertise_subject(&code);
        let advertisement = PairingAdvertisement {
            agent_id: self.agent_id.clone(),
            code: code.clone(),
            hostname: sysinfo::System::host_name().unwrap_or_else(|| "unknown".into()),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            timestamp: Utc::now(),
        };

        let deadline = tokio::time::Instant::now()
            + tokio::time::Duration::from_secs(PAIRING_EXPIRY_SECS);

        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    tracing::info!("[PAIRING] Code {code} expired. Generating new code...");
                    // Recurse with a new code
                    return Box::pin(self.run()).await;
                }
                msg = claim_sub.next() => {
                    if let Some(msg) = msg {
                        match serde_json::from_slice::<PairingClaim>(&msg.payload) {
                            Ok(claim) => {
                                return self.handle_claim(&code, claim).await;
                            }
                            Err(e) => {
                                tracing::warn!("Invalid pairing claim payload: {e}");
                            }
                        }
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(10)) => {
                    // Publish advertisement
                    let ad = PairingAdvertisement {
                        timestamp: Utc::now(),
                        ..advertisement.clone()
                    };
                    if let Ok(payload) = serde_json::to_vec(&ad) {
                        if let Err(e) = self.client.publish(advertise_subject.clone(), payload.into()).await {
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

        // Store in SQLite
        {
            let store = self.store.lock().await;
            if let Err(e) = store.set_config("owner_id", owner_id) {
                tracing::error!("Failed to store owner_id: {e}");
                self.send_ack(code, false, "Failed to store owner_id").await;
                return None;
            }
            let _ = store.set_config("paired_at", &Utc::now().to_rfc3339());
        }

        // Write to agent.env for systemd restart persistence
        self.write_owner_to_env(owner_id);

        // Send ack
        self.send_ack(code, true, "Paired successfully").await;

        tracing::info!(
            "[PAIRING] Paired with owner {owner_id}. Agent will now use scoped subjects."
        );

        Some(PairingResult {
            owner_id: owner_id.clone(),
        })
    }

    async fn send_ack(&self, code: &str, success: bool, message: &str) {
        let ack = PairingAck {
            success,
            agent_id: self.agent_id.clone(),
            owner_id: String::new(),
            message: message.into(),
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
            tracing::warn!("agent.env not found at {}, skipping env write", env_path.display());
            return;
        }

        match std::fs::read_to_string(env_path) {
            Ok(content) => {
                let new_line = format!("BERTH_OWNER_ID={owner_id}");
                let new_content = if content.contains("BERTH_OWNER_ID=") {
                    // Replace existing line
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
                    // Append
                    format!("{content}\n{new_line}")
                };
                if let Err(e) = std::fs::write(env_path, new_content) {
                    tracing::warn!("Failed to write BERTH_OWNER_ID to {}: {e}", env_path.display());
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read {}: {e}", env_path.display());
            }
        }
    }
}
