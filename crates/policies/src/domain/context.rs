use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use shared::hrn::{Hrn, UserId};

use crate::domain::action::Action;
use shared::attributes::AttributeValue;

/// Contexto para la evaluación de una política
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEvaluationContext {
    pub principal: UserId,
    pub action: Action,
    pub resource: Hrn,
    pub time: DateTime<Utc>,
    pub additional_attributes: HashMap<String, AttributeValue>,
}

impl PolicyEvaluationContext {
    pub fn new(principal: UserId, action: Action, resource: Hrn, time: DateTime<Utc>) -> Self {
        Self { principal, action, resource, time, additional_attributes: HashMap::new() }
    }
}
