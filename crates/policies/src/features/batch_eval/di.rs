use anyhow::Result;

use super::use_case::BatchEvalUseCase;

pub async fn make_use_case_mem() -> Result<BatchEvalUseCase> {
    Ok(BatchEvalUseCase::new())
}
