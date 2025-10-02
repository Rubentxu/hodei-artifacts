use anyhow::Result;

use super::use_case::AnalyzePoliciesUseCase;

pub async fn make_use_case_mem() -> Result<AnalyzePoliciesUseCase> {
    Ok(AnalyzePoliciesUseCase::new())
}
