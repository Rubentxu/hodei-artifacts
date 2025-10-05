use async_trait::async_trait;
use cedar_policy::Policy;
use kernel::Hrn;
use std::str::FromStr;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::opt::RecordId;

/// Policy entity for IAM
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IamPolicy {
    pub hrn: Hrn,
    pub name: String,
    pub policy_text: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

impl IamPolicy {
    /// Create a new IAM policy
    pub fn new(hrn: Hrn, name: String, policy_text: String) -> Self {
        Self {
            hrn,
            name,
            policy_text,
            description: None,
            tags: Vec::new(),
        }
    }

    /// Parse the policy text into a Cedar Policy
    pub fn as_cedar_policy(&self) -> Result<Policy, cedar_policy::ParseErrors> {
        Policy::from_str(&self.policy_text)
    }
}

/// Repository for IAM policies
pub struct IamPolicyRepository {
    db: Surreal<Any>,
}

impl IamPolicyRepository {
    /// Create a new IamPolicyRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl IamPolicyRepository {
    /// Save a policy
    pub async fn save(&self, policy: &IamPolicy) -> Result<(), anyhow::Error> {
        let thing: RecordId = ("iam_policies", policy.hrn.to_string()).try_into()?;
        let _: surrealdb::opt::IntoRecordId = self.db.create(thing).content(policy).await?;
        Ok(())
    }

    /// Find policy by HRN
    pub async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<IamPolicy>, anyhow::Error> {
        let thing: RecordId = ("iam_policies", hrn.to_string()).try_into()?;
        let policy: Option<IamPolicy> = self.db.select(thing).await?;
        Ok(policy)
    }

    /// Find all policies
    pub async fn find_all(&self) -> Result<Vec<IamPolicy>, anyhow::Error> {
        let policies: Vec<IamPolicy> = self.db.select("iam_policies").await?;
        Ok(policies)
    }

    /// Find policies by HRNs
    pub async fn find_by_hrns(&self, hrns: &[Hrn]) -> Result<Vec<IamPolicy>, anyhow::Error> {
        let mut policies = Vec::new();
        for hrn in hrns {
            if let Some(policy) = self.find_by_hrn(hrn).await? {
                policies.push(policy);
            }
        }
        Ok(policies)
    }

    /// Delete a policy
    pub async fn delete(&self, hrn: &Hrn) -> Result<bool, anyhow::Error> {
        let thing: RecordId = ("iam_policies", hrn.to_string()).try_into()?;
        let result: Option<IamPolicy> = self.db.delete(thing).await?;
        Ok(result.is_some())
    }
}
