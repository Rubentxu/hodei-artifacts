use serde::{Serialize, Deserialize};
use shared::{IsoTimestamp, UserId};

// Value Object simple para nombres de métricas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricName(pub String);
impl MetricName { pub fn new(raw: impl Into<String>) -> Self { Self(raw.into()) } }

// Value Object para un valor numérico (podrá evolucionar a enum / variant multi-tipo)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue(pub f64);
impl MetricValue { pub fn new(v: f64) -> Self { Self(v) } }

// Entidad básica de un dato analítico atómico registrado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageMetric {
    pub name: MetricName,
    pub value: MetricValue,
    pub recorded_at: IsoTimestamp,
    pub recorded_by: Option<UserId>,
}

impl UsageMetric {
    pub fn new(name: MetricName, value: MetricValue, recorded_by: Option<UserId>) -> Self {
        Self { name, value, recorded_at: IsoTimestamp::now(), recorded_by }
    }
}

