//! Domain events for IAM bounded context
//!
//! Following VSA and Event-Driven Architecture principles:
//! - Events represent facts that have occurred in the past
//! - Events are immutable and named in past tense
//! - Used for communication between bounded contexts

use serde::{Deserialize, Serialize};
use shared::domain::event::DomainEvent;
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// User lifecycle events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCreated {
    pub user_id: Uuid,
    pub external_id: String,
    pub user_type: String,
    pub attributes: std::collections::HashMap<String, String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl DomainEvent for UserCreated {
    fn event_type(&self) -> &'static str {
        "UserCreated"
    }

    fn aggregate_id(&self) -> String {
        self.user_id.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdated {
    pub user_id: Uuid,
    pub changed_attributes: std::collections::HashMap<String, String>,
    pub updated_by: Uuid,
    pub updated_at: DateTime<Utc>,
    pub reason: Option<String>,
}

impl DomainEvent for UserUpdated {
    fn event_type(&self) -> &'static str {
        "UserUpdated"
    }

    fn aggregate_id(&self) -> String {
        self.user_id.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDeactivated {
    pub user_id: Uuid,
    pub deactivated_by: Uuid,
    pub deactivated_at: DateTime<Utc>,
    pub reason: String,
    pub last_activity: Option<DateTime<Utc>>,
}

impl DomainEvent for UserDeactivated {
    fn event_type(&self) -> &'static str {
        "UserDeactivated"
    }

    fn aggregate_id(&self) -> String {
        self.user_id.to_string()
    }
}

/// Policy management events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCreated {
    pub policy_id: Uuid,
    pub policy_text: String,
    pub created_by: Uuid,
    pub scope: Vec<String>,
    pub version: String,
    pub created_at: DateTime<Utc>,
}

impl DomainEvent for PolicyCreated {
    fn event_type(&self) -> &'static str {
        "PolicyCreated"
    }

    fn aggregate_id(&self) -> String {
        self.policy_id.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdated {
    pub policy_id: Uuid,
    pub old_version: String,
    pub new_version: String,
    pub changes: Vec<String>,
    pub updated_by: Uuid,
    pub updated_at: DateTime<Utc>,
}

impl DomainEvent for PolicyUpdated {
    fn event_type(&self) -> &'static str {
        "PolicyUpdated"
    }

    fn aggregate_id(&self) -> String {
        self.policy_id.to_string()
    }
}

/// Access control events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessDecisionMade {
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub decision: bool,
    pub applied_policies: Vec<String>,
    pub evaluation_time_ms: u64,
    pub decided_at: DateTime<Utc>,
}

impl DomainEvent for AccessDecisionMade {
    fn event_type(&self) -> &'static str {
        "AccessDecisionMade"
    }

    fn aggregate_id(&self) -> String {
        format!("{}:{}:{}", self.principal, self.action, self.resource)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspiciousAccessAttempt {
    pub principal: String,
    pub resource: String,
    pub attempt_details: std::collections::HashMap<String, String>,
    pub risk_score: f64,
    pub blocked_by: String,
    pub detected_at: DateTime<Utc>,
}

impl DomainEvent for SuspiciousAccessAttempt {
    fn event_type(&self) -> &'static str {
        "SuspiciousAccessAttempt"
    }

    fn aggregate_id(&self) -> String {
        format!("{}:{}", self.principal, self.resource)
    }
}
