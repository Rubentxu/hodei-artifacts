use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tipo de atributo genérico para representar datos de recursos/principales
/// y contextos de autorización, independiente del motor (p.ej. Cedar).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    String(String),
    Long(i64),
    Boolean(bool),
    Set(Vec<AttributeValue>),
    Record(HashMap<String, AttributeValue>),
}
