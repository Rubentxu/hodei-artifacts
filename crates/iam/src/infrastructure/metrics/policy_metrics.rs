use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MetricValue {
    pub value: f64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub struct PolicyMetrics {
    metrics: Arc<RwLock<HashMap<String, Vec<MetricValue>>>>,
}

impl PolicyMetrics {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn record_metric(&self, name: &str, value: f64) {
        let mut metrics = self.metrics.write().await;
        let metric_value = MetricValue {
            value,
            timestamp: chrono::Utc::now(),
        };
        
        metrics.entry(name.to_string())
            .or_insert_with(Vec::new)
            .push(metric_value);
    }

    pub async fn get_metric(&self, name: &str) -> Option<Vec<MetricValue>> {
        let metrics = self.metrics.read().await;
        metrics.get(name).cloned()
    }

    // Policy validation metrics
    pub async fn record_validation_duration(&self, duration_ms: u64) {
        self.record_metric("validation_duration_ms", duration_ms as f64).await;
    }

    pub async fn record_validation_success(&self) {
        self.record_metric("validation_success", 1.0).await;
    }

    pub async fn record_validation_failure(&self) {
        self.record_metric("validation_failure", 1.0).await;
    }

    // Policy conflict detection metrics
    pub async fn record_conflict_detection_duration(&self, duration_ms: u64) {
        self.record_metric("conflict_detection_duration_ms", duration_ms as f64).await;
    }

    pub async fn record_conflicts_found(&self, count: usize) {
        self.record_metric("conflicts_found", count as f64).await;
    }

    // Policy coverage analysis metrics
    pub async fn record_coverage_analysis_duration(&self, duration_ms: u64) {
        self.record_metric("coverage_analysis_duration_ms", duration_ms as f64).await;
    }

    pub async fn record_coverage_percentage(&self, percentage: f64) {
        self.record_metric("coverage_percentage", percentage).await;
    }

    pub async fn record_coverage_gaps_found(&self, gap_count: usize) {
        self.record_metric("coverage_gaps_found", gap_count as f64).await;
    }

    pub async fn record_coverage_suggestions_generated(&self, suggestion_count: usize) {
        self.record_metric("coverage_suggestions_generated", suggestion_count as f64).await;
    }

    // General policy metrics
    pub async fn record_policy_created(&self) {
        self.record_metric("policy_created", 1.0).await;
    }

    pub async fn record_policy_updated(&self) {
        self.record_metric("policy_updated", 1.0).await;
    }

    pub async fn record_policy_deleted(&self) {
        self.record_metric("policy_deleted", 1.0).await;
    }

    pub async fn record_policy_retrieved(&self) {
        self.record_metric("policy_retrieved", 1.0).await;
    }

    // Batch operation metrics
    pub async fn record_batch_validation_duration(&self, duration_ms: u64) {
        self.record_metric("batch_validation_duration_ms", duration_ms as f64).await;
    }

    pub async fn record_batch_size(&self, size: usize) {
        self.record_metric("batch_size", size as f64).await;
    }

    pub async fn record_batch_success_rate(&self, success_rate: f64) {
        self.record_metric("batch_success_rate", success_rate).await;
    }
}

impl Default for PolicyMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_and_get_metric() {
        let metrics = PolicyMetrics::new();
        
        metrics.record_validation_duration(100).await;
        metrics.record_validation_success().await;
        
        let validation_duration = metrics.get_metric("validation_duration_ms").await;
        assert!(validation_duration.is_some());
        assert_eq!(validation_duration.unwrap().len(), 1);
        
        let validation_success = metrics.get_metric("validation_success").await;
        assert!(validation_success.is_some());
        assert_eq!(validation_success.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_coverage_metrics() {
        let metrics = PolicyMetrics::new();
        
        metrics.record_coverage_analysis_duration(500).await;
        metrics.record_coverage_percentage(85.5).await;
        metrics.record_coverage_gaps_found(3).await;
        metrics.record_coverage_suggestions_generated(5).await;
        
        let duration = metrics.get_metric("coverage_analysis_duration_ms").await;
        assert!(duration.is_some());
        assert_eq!(duration.unwrap()[0].value, 500.0);
        
        let percentage = metrics.get_metric("coverage_percentage").await;
        assert!(percentage.is_some());
        assert_eq!(percentage.unwrap()[0].value, 85.5);
    }
}