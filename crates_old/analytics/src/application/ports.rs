use async_trait::async_trait;
use crate::domain::model::UsageMetric;
use crate::domain::event::UsageMetricRecorded;
use crate::error::AnalyticsError;

#[async_trait]
pub trait UsageMetricRepository: Send + Sync {
    async fn save(&self, metric: &UsageMetric) -> Result<(), AnalyticsError>;
}

#[async_trait]
pub trait UsageMetricEventPublisher: Send + Sync {
    async fn publish_recorded(&self, event: &UsageMetricRecorded) -> Result<(), AnalyticsError>;
}

