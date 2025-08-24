use crate::application::ports::SearchIndex;
use crate::domain::model::ArtifactSearchDocument;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message;
use shared::domain::event::ArtifactUploadedEvent;
use shared::domain::model::IsoTimestamp;
use std::sync::Arc;
use tracing::{error, info};

pub struct KafkaEventConsumer {
    consumer: StreamConsumer,
    search_index: Arc<dyn SearchIndex>,
}

impl KafkaEventConsumer {
    pub fn new(
        bootstrap_servers: &str,
        group_id: &str,
        search_index: Arc<dyn SearchIndex>,
    ) -> Self {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("group.id", group_id)
            .set("auto.offset.reset", "earliest")
            .create()
            .expect("Consumer creation failed");

        Self {
            consumer,
            search_index,
        }
    }

    pub async fn run(&self, topic: &str) {
        self.consumer
            .subscribe(&[topic])
            .expect("Can't subscribe to specified topic");

        loop {
            match self.consumer.recv().await {
                Ok(msg) => {
                    let payload = match msg.payload_view::<str>() {
                        None => "",
                        Some(Ok(s)) => s,
                        Some(Err(e)) => {
                            error!("Error viewing payload: {}", e);
                            continue;
                        }
                    };

                    let envelope: ArtifactUploadedEvent = match serde_json::from_str(payload) {
                        Ok(envelope) => envelope,
                        Err(e) => {
                            error!("Failed to deserialize event envelope: {}", e);
                            continue;
                        }
                    };

                    let event = envelope.data;

                    let document = ArtifactSearchDocument {
                        artifact_id: event.artifact_id,
                        repository_id: event.repository_id,
                        name: event.artifact_id.to_string(), // Placeholder
                        version: "unknown".to_string(),       // Placeholder
                        description: None,
                        tags: vec![],
                        indexed_at: IsoTimestamp::now(),
                    };

                    if let Err(e) = self.search_index.index(&document).await {
                        error!("Failed to index document: {}", e);
                    } else {
                        info!("Indexed document for artifact {}", document.artifact_id);
                    }
                }
                Err(e) => error!("Kafka error: {}", e),
            }
        }
    }
}
