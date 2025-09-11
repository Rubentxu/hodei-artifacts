// Distribution Crate
pub mod domain;
pub mod features;
pub mod infrastructure;

pub type DistributionResult<T> = Result<T, infrastructure::errors::DistributionInfrastructureError>;
