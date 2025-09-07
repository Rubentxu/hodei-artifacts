use serde::{Serialize, Deserialize};
use shared::{IsoTimestamp, UserId};
use crate::domain::model::{MetricName, MetricValue};

// Evento de dominio: se ha registrado una métrica de uso básica.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageMetricRecorded {
    pub name: MetricName,
    pub value: MetricValue,
    pub recorded_at: IsoTimestamp,
    pub recorded_by: Option<UserId>,
}

impl UsageMetricRecorded {
    pub fn new(name: MetricName, value: MetricValue, recorded_by: Option<UserId>) -> Self {
        Self { name, value, recorded_at: IsoTimestamp::now(), recorded_by }
    }
}

