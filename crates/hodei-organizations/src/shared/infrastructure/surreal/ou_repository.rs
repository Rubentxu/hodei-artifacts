use crate::shared::application::ports::ou_repository::{OuRepository, OuRepositoryError};
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

pub struct SurrealOuRepository {
    db: Surreal<Any>,
}

impl SurrealOuRepository {
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OuRepository for SurrealOuRepository {
    async fn save(&self, ou: &OrganizationalUnit) -> Result<(), OuRepositoryError> {
        let hrn_str = ou.hrn.to_string();
        let _: Option<OrganizationalUnit> = self
            .db
            .create(("ou", &hrn_str))
            .content(ou.clone())
            .await
            .map_err(|e| OuRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<OrganizationalUnit> = self
            .db
            .select(("ou", &hrn_str))
            .await
            .map_err(|e| OuRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}
