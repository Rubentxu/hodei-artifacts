use anyhow::Result;

use super::use_case::TracedPlaygroundUseCase;

pub async fn make_use_case_mem() -> Result<TracedPlaygroundUseCase> {
    Ok(TracedPlaygroundUseCase::new())
}
