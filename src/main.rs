use hodei_artifacts_api::{bootstrap, Application};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = bootstrap().await?;
    let application = Application::new(8080, app_state).await;
    application.run().await?;
    Ok(())
}
