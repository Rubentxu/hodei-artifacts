// crates/search/src/infrastructure/cedar_adapter.rs

use cedar_policy::{EntityUid, RestrictedExpression, EntityTypeName, EntityId};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::dashboard::Dashboard;
use crate::domain::report::Report;
use crate::domain::alert::Alert;
use shared::security::HodeiResource;

/// Cedar Policy adapter implementation for Dashboard
impl HodeiResource<EntityUid, RestrictedExpression> for Dashboard {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(self.id.0.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("Dashboard").unwrap();
            let entity_id = EntityId::from_str(self.id.0.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("dashboard".to_string()));
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        attrs.insert("public".to_string(), RestrictedExpression::new_bool(self.public));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        vec![EntityUid::from_str(self.organization.0.as_str()).unwrap()]
    }
}

/// Cedar Policy adapter implementation for Report
impl HodeiResource<EntityUid, RestrictedExpression> for Report {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(self.id.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("Report").unwrap();
            let entity_id = EntityId::from_str(self.id.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("report".to_string()));
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        attrs.insert("report_type".to_string(), RestrictedExpression::new_string(self.r#type.clone()));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // Reports may be organization-scoped or user-scoped; default to organization
        Vec::new() // Adjust if we add organization field later
    }
}

/// Cedar Policy adapter implementation for Alert
impl HodeiResource<EntityUid, RestrictedExpression> for Alert {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(self.id.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("Alert").unwrap();
            let entity_id = EntityId::from_str(self.id.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("alert".to_string()));
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        attrs.insert("severity".to_string(), RestrictedExpression::new_string(format!("{:?}", self.severity)));
        attrs.insert("active".to_string(), RestrictedExpression::new_bool(self.active));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        Vec::new() // Alerts may be system-wide or organization-scoped; adjust if needed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{DashboardId, OrganizationId, UserId};
    use time::OffsetDateTime;

    fn dashboard() -> Dashboard {
        Dashboard {
            id: DashboardId(shared::hrn::Hrn::new("hrn:hodei:search:global:acme:dashboard/main").unwrap()),
            name: "main".into(),
            organization: OrganizationId(shared::hrn::Hrn::new("hrn:hodei:iam:global:acme:organization").unwrap()),
            widgets: std::collections::HashSet::new(),
            public: false,
            created_at: OffsetDateTime::now_utc(),
            updated_at: None,
        }
    }

    fn report() -> Report {
        Report {
            id: shared::hrn::Hrn::new("hrn:hodei:search:global:acme:report/weekly").unwrap(),
            name: "weekly".into(),
            r#type: "usage".into(),
            data: "{}".into(),
            generated_at: OffsetDateTime::now_utc(),
            generated_by: UserId(shared::hrn::Hrn::new("hrn:hodei:iam:global:acme:user/admin").unwrap()),
        }
    }

    fn alert() -> Alert {
        Alert {
            id: shared::hrn::Hrn::new("hrn:hodei:search:global:acme:alert/high-usage").unwrap(),
            name: "high-usage".into(),
            condition: "usage > 90%".into(),
            severity: crate::domain::alert::Severity::High,
            active: true,
            created_at: OffsetDateTime::now_utc(),
            updated_at: None,
        }
    }

    #[test]
    fn dashboard_adapter_smoke() {
        let d = dashboard();
        let id = <Dashboard as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&d);
        let attrs = <Dashboard as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&d);
        let parents = <Dashboard as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&d);
        assert!(!id.to_string().is_empty());
        assert!(attrs.contains_key("name"));
        assert_eq!(parents.len(), 1);
    }

    #[test]
    fn report_adapter_smoke() {
        let r = report();
        let id = <Report as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&r);
        let attrs = <Report as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&r);
        let parents = <Report as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&r);
        assert!(!id.to_string().is_empty());
        assert!(attrs.contains_key("report_type"));
        assert!(parents.is_empty());
    }

    #[test]
    fn alert_adapter_smoke() {
        let a = alert();
        let id = <Alert as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&a);
        let attrs = <Alert as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&a);
        let parents = <Alert as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&a);
        assert!(!id.to_string().is_empty());
        assert!(attrs.contains_key("severity"));
        assert!(parents.is_empty());
    }
}
