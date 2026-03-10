use std::pin::Pin;

use async_nats::jetstream;
use futures::stream::{Stream, StreamExt};
use tokio::sync::mpsc;

use berth_proto::nats_relay::{
    self, NatsConfig, NatsEvent, NatsHeartbeat, NatsLogLine,
};

pub struct NatsSubscriber {
    client: async_nats::Client,
    jetstream: jetstream::Context,
    install_id: String,
}

impl NatsSubscriber {
    pub async fn connect(config: &NatsConfig, install_id: &str) -> anyhow::Result<Self> {
        let mut opts = async_nats::ConnectOptions::new();
        if let Some(ref creds_path) = config.creds_path {
            opts = opts.credentials_file(creds_path).await?;
        }

        let client = opts.connect(&config.url).await?;
        let jetstream = jetstream::new(client.clone());

        tracing::info!(
            install_id,
            url = config.url,
            "NATS subscriber connected"
        );

        Ok(Self {
            client,
            jetstream,
            install_id: install_id.to_string(),
        })
    }

    pub fn client(&self) -> &async_nats::Client {
        &self.client
    }

    pub async fn subscribe_events(
        &self,
        agent_ids: &[String],
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = NatsEvent> + Send>>> {
        let stream = self
            .jetstream
            .get_stream("BERTH_EVENTS")
            .await?;

        let filter_subjects: Vec<String> = if agent_ids.is_empty() {
            vec![format!("berth.{}.*.event.>", self.install_id)]
        } else {
            agent_ids
                .iter()
                .map(|id| format!("berth.{}.{id}.event.>", self.install_id))
                .collect()
        };

        let consumer_name = format!("desktop-{}", self.install_id);
        let consumer = stream
            .get_or_create_consumer(
                &consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.clone()),
                    filter_subjects,
                    ack_policy: jetstream::consumer::AckPolicy::Explicit,
                    ..Default::default()
                },
            )
            .await?;

        let messages = consumer.messages().await?;

        let stream = messages.filter_map(|msg| async {
            match msg {
                Ok(msg) => {
                    let payload = msg.payload.as_ref();
                    match serde_json::from_slice::<NatsEvent>(payload) {
                        Ok(event) => {
                            if let Err(e) = msg.ack().await {
                                tracing::warn!("Failed to ack NATS event: {e}");
                            }
                            Some(event)
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse NATS event: {e}");
                            let _ = msg.ack().await;
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("NATS event stream error: {e}");
                    None
                }
            }
        });

        Ok(Box::pin(stream))
    }

    pub async fn subscribe_logs(
        &self,
        agent_id: &str,
        project_id: &str,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = NatsLogLine> + Send>>> {
        let subject = nats_relay::log_subject(&self.install_id, agent_id, project_id);

        let stream = self
            .jetstream
            .get_stream("BERTH_LOGS")
            .await?;

        let consumer_name = format!("desktop-{}-logs-{}-{}", self.install_id, agent_id, project_id);
        let consumer = stream
            .get_or_create_consumer(
                &consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.clone()),
                    filter_subjects: vec![subject],
                    ack_policy: jetstream::consumer::AckPolicy::None,
                    deliver_policy: jetstream::consumer::DeliverPolicy::New,
                    ..Default::default()
                },
            )
            .await?;

        let messages = consumer.messages().await?;

        let stream = messages.filter_map(|msg| async {
            match msg {
                Ok(msg) => {
                    match serde_json::from_slice::<NatsLogLine>(msg.payload.as_ref()) {
                        Ok(log_line) => Some(log_line),
                        Err(e) => {
                            tracing::warn!("Failed to parse NATS log line: {e}");
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("NATS log stream error: {e}");
                    None
                }
            }
        });

        Ok(Box::pin(stream))
    }

    pub async fn subscribe_heartbeats(
        &self,
        agent_ids: &[String],
    ) -> anyhow::Result<mpsc::UnboundedReceiver<NatsHeartbeat>> {
        let (tx, rx) = mpsc::unbounded_channel();

        for agent_id in agent_ids {
            let subject = nats_relay::heartbeat_subject(&self.install_id, agent_id);
            let mut sub = self.client.subscribe(subject).await?;
            let tx = tx.clone();

            tokio::spawn(async move {
                while let Some(msg) = sub.next().await {
                    match serde_json::from_slice::<NatsHeartbeat>(msg.payload.as_ref()) {
                        Ok(hb) => {
                            if tx.send(hb).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse heartbeat: {e}");
                        }
                    }
                }
            });
        }

        Ok(rx)
    }
}
