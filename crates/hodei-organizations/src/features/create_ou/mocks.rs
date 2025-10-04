use crate::features::create_ou::ports::OuPersister;
use crate::features::create_ou::error::CreateOuError;
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Arc;

pub struct MockOuPersister {
    ous: Arc<Mutex<HashMap<String, OrganizationalUnit>>>,
}

impl MockOuPersister {
    pub fn new() -> Self {
        Self {
            ous: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl OuPersister for MockOuPersister {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError> {
        let mut ous = self.ous.lock().unwrap();
        ous.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}
