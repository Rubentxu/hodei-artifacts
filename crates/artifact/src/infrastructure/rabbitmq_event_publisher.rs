use async_trait::async_trait;
use lapin::{options::*, types::FieldTable, BasicProperties, Connection, ConnectionProperties};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::application::ports::ArtifactEventPublisher;
use shared::domain::event::ArtifactDownloadRequestedEvent;
use crate::error::ArtifactError;
use anyhow::Result;

#[derive(Clone)]
pub struct RabbitMqArtifactEventPublisher {
    connection: Arc<Mutex<Connection>>,
    exchange_name: String,
}

impl RabbitMqArtifactEventPublisher {
    pub async fn new(amqp_addr: &str, exchange_name: &str) -> Result<Self> {
        let connection = Connection::connect(
            amqp_addr,
            ConnectionProperties::default()
                .with_connection_name("hodei_artifacts_publisher".into())
        ).await?;

        // Confirm connection is open
        connection.create_channel().await?.confirm_select(ConfirmSelectOptions::default()).await?;

        Ok(RabbitMqArtifactEventPublisher {
            connection: Arc::new(Mutex::new(connection)),
            exchange_name: exchange_name.to_string(),
        })
    }
}

#[async_trait]
impl ArtifactEventPublisher for RabbitMqArtifactEventPublisher {
    async fn publish_created(&self, event: &shared::domain::event::ArtifactUploadedEvent) -> Result<(), ArtifactError> {
        let connection = self.connection.lock().await;
        let channel = connection.create_channel().await.map_err(|e| ArtifactError::EventPublishing { event_type: "rabbitmq_channel_create".to_string(), source: Box::new(e) })?;

        let event_json = serde_json::to_vec(event).map_err(|e| ArtifactError::EventPublishing { event_type: "serde_json_serialize".to_string(), source: Box::new(e) })?;

        channel.exchange_declare(
            &self.exchange_name,
            lapin::ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        ).await.map_err(|e| ArtifactError::EventPublishing { event_type: "rabbitmq_exchange_declare".to_string(), source: Box::new(e) })?;

        channel.basic_publish(
            &self.exchange_name,
            "artifact.uploaded", // Routing key
            BasicPublishOptions::default(),
            &event_json, // Fix: Pass reference
            BasicProperties::default().with_content_type("application/json".into()),
        ).await.map_err(|e| ArtifactError::EventPublishing { event_type: "rabbitmq_basic_publish".to_string(), source: Box::new(e) })?;

        println!("Published ArtifactUploadedEvent to RabbitMQ: {:?}", event);
        Ok(())
    }

    async fn publish_download_requested(&self, event: &ArtifactDownloadRequestedEvent) -> Result<(), ArtifactError> {
        let connection = self.connection.lock().await;
        let channel = connection.create_channel().await.map_err(|e| ArtifactError::EventPublishing { event_type: "rabbitmq_channel_create".to_string(), source: Box::new(e) })?;

        let payload = serde_json::to_vec(event).map_err(|e| ArtifactError::EventPublishing { event_type: "serde_json_serialize".to_string(), source: Box::new(e) })?;

        channel.exchange_declare(
            &self.exchange_name,
            lapin::ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        ).await.map_err(|e| ArtifactError::EventPublishing { event_type: "rabbitmq_exchange_declare".to_string(), source: Box::new(e) })?;

        channel.basic_publish(
            &self.exchange_name,
            "artifact.download.requested", // Routing key
            BasicPublishOptions::default(),
            &payload, // Fix: Pass reference
            BasicProperties::default().with_content_type("application/json".into()),
        ).await.map_err(|e| ArtifactError::EventPublishing { event_type: "rabbitmq_basic_publish".to_string(), source: Box::new(e) })?;

        println!("Published ArtifactDownloadRequestedEvent to RabbitMQ: {:?}", event);
        Ok(())
    }
}
