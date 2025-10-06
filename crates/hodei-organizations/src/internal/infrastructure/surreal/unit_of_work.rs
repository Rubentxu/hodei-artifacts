use async_trait::async_trait;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

use kernel::application::ports::unit_of_work::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};

use crate::internal::application::ports::account_repository::AccountRepository;
use crate::internal::application::ports::ou_repository::OuRepository;
use crate::internal::application::ports::scp_repository::ScpRepository;

/// Transactional account repository that operates within a UnitOfWork context
pub struct TransactionalAccountRepository<C = Any>
where
    C: surrealdb::Connection,
{
    db: Arc<Surreal<C>>,
}

impl<C> TransactionalAccountRepository<C>
where
    C: surrealdb::Connection,
{
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl<C> AccountRepository for TransactionalAccountRepository<C>
where
    C: surrealdb::Connection,
{
    async fn save(
        &self,
        account: &crate::internal::domain::account::Account,
    ) -> Result<(), crate::internal::application::ports::account_repository::AccountRepositoryError>
    {
        let hrn_str = account.hrn.to_string();
        self.db.create::<Option<crate::internal::domain::account::Account>>(("account", &hrn_str)).content(account.clone()).await
            .map_err(|e| crate::internal::application::ports::account_repository::AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &kernel::Hrn,
    ) -> Result<
        Option<crate::internal::domain::account::Account>,
        crate::internal::application::ports::account_repository::AccountRepositoryError,
    > {
        let hrn_str = hrn.to_string();
        let result: Option<crate::internal::domain::account::Account> = self.db.select(("account", &hrn_str)).await
            .map_err(|e| crate::internal::application::ports::account_repository::AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}

/// Transactional organizational unit repository that operates within a UnitOfWork context
pub struct TransactionalOuRepository<C = Any>
where
    C: surrealdb::Connection,
{
    db: Arc<Surreal<C>>,
}

impl<C> TransactionalOuRepository<C>
where
    C: surrealdb::Connection,
{
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl<C> OuRepository for TransactionalOuRepository<C>
where
    C: surrealdb::Connection,
{
    async fn save(
        &self,
        ou: &crate::internal::domain::ou::OrganizationalUnit,
    ) -> Result<(), crate::internal::application::ports::ou_repository::OuRepositoryError> {
        let hrn_str = ou.hrn.to_string();
        self.db
            .create::<Option<crate::internal::domain::ou::OrganizationalUnit>>(("ou", &hrn_str))
            .content(ou.clone())
            .await
            .map_err(|e| {
                crate::internal::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    e.to_string(),
                )
            })?;
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &kernel::Hrn,
    ) -> Result<
        Option<crate::internal::domain::ou::OrganizationalUnit>,
        crate::internal::application::ports::ou_repository::OuRepositoryError,
    > {
        let hrn_str = hrn.to_string();
        let result: Option<crate::internal::domain::ou::OrganizationalUnit> =
            self.db.select(("ou", &hrn_str)).await.map_err(|e| {
                crate::internal::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    e.to_string(),
                )
            })?;
        Ok(result)
    }
}

/// Transactional service control policy repository that operates within a UnitOfWork context
pub struct TransactionalScpRepository<C = Any>
where
    C: surrealdb::Connection,
{
    db: Arc<Surreal<C>>,
}

impl<C> TransactionalScpRepository<C>
where
    C: surrealdb::Connection,
{
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ScpRepository for TransactionalScpRepository {
    async fn save(
        &self,
        scp: &crate::internal::domain::scp::ServiceControlPolicy,
    ) -> Result<(), crate::internal::application::ports::scp_repository::ScpRepositoryError> {
        let hrn_str = scp.hrn.to_string();
        self.db
            .create::<Option<crate::internal::domain::scp::ServiceControlPolicy>>(("scp", &hrn_str))
            .content(scp.clone())
            .await
            .map_err(|e| {
                crate::internal::application::ports::scp_repository::ScpRepositoryError::Storage(
                    e.to_string(),
                )
            })?;
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &kernel::Hrn,
    ) -> Result<
        Option<crate::internal::domain::scp::ServiceControlPolicy>,
        crate::internal::application::ports::scp_repository::ScpRepositoryError,
    > {
        let hrn_str = hrn.to_string();
        let result: Option<crate::internal::domain::scp::ServiceControlPolicy> =
            self.db.select(("scp", &hrn_str)).await.map_err(|e| {
                crate::internal::application::ports::scp_repository::ScpRepositoryError::Storage(
                    e.to_string(),
                )
            })?;
        Ok(result)
    }
}

/// SurrealDB implementation of UnitOfWork
///
/// This implementation manages database transactions and provides transactional
/// repository instances that automatically participate in the transaction context.
pub struct SurrealUnitOfWork<C = Any>
where
    C: surrealdb::Connection,
{
    db: Arc<Surreal<C>>,
    transaction_started: bool,
}

impl<C> SurrealUnitOfWork<C>
where
    C: surrealdb::Connection,
{
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self {
            db,
            transaction_started: false,
        }
    }
}

#[async_trait]
impl<C> UnitOfWork for SurrealUnitOfWork<C>
where
    C: surrealdb::Connection,
{
    type AccountRepository = TransactionalAccountRepository<C>;
    type OuRepository = TransactionalOuRepository<C>;
    type ScpRepository = TransactionalScpRepository<C>;

    async fn begin(&mut self) -> Result<(), UnitOfWorkError> {
        if self.transaction_started {
            return Err(UnitOfWorkError::Transaction(
                "Transaction already started".to_string(),
            ));
        }

        self.db
            .query("BEGIN TRANSACTION")
            .await
            .map_err(|e| UnitOfWorkError::Transaction(e.to_string()))?;

        self.transaction_started = true;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), UnitOfWorkError> {
        if !self.transaction_started {
            return Err(UnitOfWorkError::Transaction(
                "No transaction in progress".to_string(),
            ));
        }

        self.db
            .query("COMMIT TRANSACTION")
            .await
            .map_err(|e| UnitOfWorkError::CommitFailed(e.to_string()))?;

        self.transaction_started = false;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), UnitOfWorkError> {
        if !self.transaction_started {
            return Err(UnitOfWorkError::Transaction(
                "No transaction in progress".to_string(),
            ));
        }

        self.db
            .query("CANCEL TRANSACTION")
            .await
            .map_err(|e| UnitOfWorkError::RollbackFailed(e.to_string()))?;

        self.transaction_started = false;
        Ok(())
    }

    fn accounts(&self) -> Arc<Self::AccountRepository> {
        Arc::new(TransactionalAccountRepository::new(self.db.clone()))
    }

    fn ous(&self) -> Arc<Self::OuRepository> {
        Arc::new(TransactionalOuRepository::new(self.db.clone()))
    }

    fn scps(&self) -> Arc<Self::ScpRepository> {
        Arc::new(TransactionalScpRepository::new(self.db.clone()))
    }
}

impl<C> Drop for SurrealUnitOfWork<C>
where
    C: surrealdb::Connection,
{
    fn drop(&mut self) {
        if self.transaction_started {
            // Auto-rollback on drop if transaction is still active
            // Note: This is a best-effort cleanup; in async context, we can't
            // guarantee the rollback completes, but we attempt to cancel
            let db = self.db.clone();
            tokio::spawn(async move {
                let _ = db.query("CANCEL TRANSACTION").await;
            });
        }
    }
}

/// Factory for creating SurrealUnitOfWork instances
pub struct SurrealUnitOfWorkFactory<C>
where
    C: surrealdb::Connection,
{
    db: Arc<Surreal<C>>,
}

impl<C> SurrealUnitOfWorkFactory<C>
where
    C: surrealdb::Connection,
{
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl<C> UnitOfWorkFactory for SurrealUnitOfWorkFactory<C>
where
    C: surrealdb::Connection,
{
    type UnitOfWork = SurrealUnitOfWork<C>;

    async fn create(&self) -> Result<Self::UnitOfWork, UnitOfWorkError> {
        Ok(SurrealUnitOfWork::new(self.db.clone()))
    }
}
