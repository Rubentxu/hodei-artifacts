use crate::features::create_ou::error::CreateOuError;
use crate::features::create_ou::ports::{
    CreateOuUnitOfWork, CreateOuUnitOfWorkFactory, OuPersister,
};
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Deprecated: Use MockCreateOuUnitOfWork instead
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

/// Mock OU Repository for testing
pub struct MockOuRepository {
    ous: Arc<Mutex<HashMap<String, OrganizationalUnit>>>,
    should_fail: bool,
}

impl MockOuRepository {
    pub fn new() -> Self {
        Self {
            ous: Arc::new(Mutex::new(HashMap::new())),
            should_fail: false,
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            ous: Arc::new(Mutex::new(HashMap::new())),
            should_fail,
        }
    }

    pub fn get_saved_ous(&self) -> Vec<OrganizationalUnit> {
        self.ous.lock().unwrap().values().cloned().collect()
    }
}

#[async_trait]
impl OuRepository for MockOuRepository {
    async fn save(
        &self,
        ou: &OrganizationalUnit,
    ) -> Result<(), crate::shared::application::ports::ou_repository::OuRepositoryError> {
        if self.should_fail {
            return Err(
                crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    "Mock failure".to_string(),
                ),
            );
        }

        let mut ous = self.ous.lock().unwrap();
        ous.insert(ou.hrn.to_string(), ou.clone());
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<
        Option<OrganizationalUnit>,
        crate::shared::application::ports::ou_repository::OuRepositoryError,
    > {
        let ous = self.ous.lock().unwrap();
        Ok(ous.get(&hrn.to_string()).cloned())
    }
}

/// Mock UnitOfWork for testing transactional behavior
pub struct MockCreateOuUnitOfWork {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<Mutex<Vec<String>>>,
    pub transaction_active: bool,
    ou_repo: Arc<MockOuRepository>,
}

impl Default for MockCreateOuUnitOfWork {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCreateOuUnitOfWork {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
            save_calls: Arc::new(Mutex::new(Vec::new())),
            transaction_active: false,
            ou_repo: Arc::new(MockOuRepository::new()),
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
            save_calls: Arc::new(Mutex::new(Vec::new())),
            transaction_active: false,
            ou_repo: Arc::new(MockOuRepository::with_failure(should_fail)),
        }
    }

    pub fn get_saved_ous(&self) -> Vec<OrganizationalUnit> {
        self.ou_repo.get_saved_ous()
    }
}

#[async_trait]
impl CreateOuUnitOfWork for MockCreateOuUnitOfWork {
    async fn begin(&mut self) -> Result<(), CreateOuError> {
        self.transaction_active = true;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), CreateOuError> {
        if !self.transaction_active {
            return Err(CreateOuError::TransactionError(
                "No transaction in progress".to_string(),
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), CreateOuError> {
        if !self.transaction_active {
            return Err(CreateOuError::TransactionError(
                "No transaction in progress".to_string(),
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    fn ous(&self) -> Arc<dyn OuRepository> {
        self.ou_repo.clone()
    }
}

/// Mock UnitOfWorkFactory for testing
pub struct MockCreateOuUnitOfWorkFactory {
    pub should_fail_on_save: bool,
}

impl Default for MockCreateOuUnitOfWorkFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCreateOuUnitOfWorkFactory {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
        }
    }
}

#[async_trait]
impl CreateOuUnitOfWorkFactory for MockCreateOuUnitOfWorkFactory {
    type UnitOfWork = MockCreateOuUnitOfWork;

    async fn create(&self) -> Result<Self::UnitOfWork, CreateOuError> {
        Ok(MockCreateOuUnitOfWork::with_failure(
            self.should_fail_on_save,
        ))
    }
}
