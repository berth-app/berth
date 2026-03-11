use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::store::ProjectStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    // Usage
    AppLaunch,
    ProjectCreate,
    ProjectRun,
    ProjectDeploy,
    ProjectDelete,
    Publish,
    ScheduleCreate,
    TemplateInstall,
    TargetAdd,
    // Errors
    RunFailed,
    DeployFailed,
    AgentConnectFailed,
    PublishFailed,
    // Crashes
    AppPanic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub id: String,
    pub event_type: String,
    pub app_version: String,
    pub os_version: String,
    pub context: serde_json::Value,
    pub occurred_at: String,
}

pub struct Telemetry {
    enabled: bool,
    device_id: String,
    app_version: String,
}

impl Telemetry {
    pub fn new(store: &ProjectStore) -> Self {
        let settings = store.get_all_settings().unwrap_or_default();

        let enabled = settings.get("telemetry_enabled").map_or(false, |v| v == "true");

        let device_id = match settings.get("telemetry_device_id") {
            Some(id) if !id.is_empty() => id.clone(),
            _ => {
                let id = Uuid::new_v4().to_string();
                let _ = store.set_setting("telemetry_device_id", &id);
                id
            }
        };

        Self {
            enabled,
            device_id,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn set_enabled(&mut self, store: &ProjectStore, enabled: bool) {
        self.enabled = enabled;
        let _ = store.set_setting("telemetry_enabled", if enabled { "true" } else { "false" });
        if enabled {
            let _ = store.set_setting("telemetry_consent_at", &Utc::now().to_rfc3339());
        }
    }

    /// Track an event. No-op if disabled.
    pub fn track(&self, store: &ProjectStore, event_type: EventType, context: serde_json::Value) {
        if !self.enabled {
            return;
        }

        let event = TelemetryEvent {
            id: Uuid::new_v4().to_string(),
            event_type: serde_json::to_value(&event_type)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            app_version: self.app_version.clone(),
            os_version: os_version(),
            context,
            occurred_at: Utc::now().to_rfc3339(),
        };

        if let Err(e) = store.insert_telemetry_event(&event) {
            tracing::warn!("Failed to record telemetry event: {e}");
        }
    }

    /// Flush pending events to the telemetry server.
    pub async fn flush(&self, store: &ProjectStore) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let events = store.get_unsynced_telemetry_events(50)?;
        if events.is_empty() {
            return Ok(());
        }

        let payload = serde_json::json!({
            "device_id": self.device_id,
            "events": events,
        });

        let client = reqwest::Client::new();
        let resp = client
            .post("https://telemetry.getberth.dev/ingest")
            .json(&payload)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?;

        if resp.status().is_success() {
            let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
            store.mark_telemetry_synced(&ids)?;
        }

        Ok(())
    }

    /// Delete all local telemetry data and reset device ID.
    pub fn purge(&mut self, store: &ProjectStore) -> Result<()> {
        store.delete_all_telemetry()?;
        let new_id = Uuid::new_v4().to_string();
        store.set_setting("telemetry_device_id", &new_id)?;
        self.device_id = new_id;
        Ok(())
    }
}

fn os_version() -> String {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}
