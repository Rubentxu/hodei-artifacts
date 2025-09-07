use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use cedar_policy::PolicyId;
use crate::application::ports::PolicyRepository;
use crate::domain::Policy;
use crate::error::IamError;

pub struct MockPolicyRepository {
    policies: Arc<Mutex<HashMap<PolicyId, Policy>>>,
}

impl MockPolicyRepository {
    pub fn new() -> Self {
        Self { policies: Arc::new(Mutex::new(HashMap::new())) }
    }
}

#[async_trait]
impl PolicyRepository for MockPolicyRepository {
    async fn find_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError> {
        let policies = self.policies.lock().unwrap();
        Ok(policies.get(id).cloned())
    }

    async fn save(&self, policy: &Policy) -> Result<(), IamError> {
        let mut policies = self.policies.lock().unwrap();
        policies.insert(policy.id.clone(), policy.clone());
        Ok(())
    }

    async fn delete(&self, id: &PolicyId) -> Result<(), IamError> {
        let mut policies = self.policies.lock().unwrap();
        policies.remove(id);
        Ok(())
    }

    async fn find_all(&self) -> Result<Vec<Policy>, IamError> {
        let policies = self.policies.lock().unwrap();
        Ok(policies.values().cloned().collect())
    }
}
