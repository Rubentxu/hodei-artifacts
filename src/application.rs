use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::state::AppState;

pub struct Application {
    port: u16,
    router: Router,
    app_state: Arc<Mutex<AppState>>,
}

impl Application {
    pub async fn new(port: u16, app_state: Arc<Mutex<AppState>>) -> Self {
        let router = crate::infrastructure::api::create_router(app_state.clone()).await;
        Self {
            port,
            router,
            app_state,
        }
    }

    pub async fn run(self) -> Result<()> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.port));
        let listener = TcpListener::bind(&addr).await?;
        tracing::info!("listening on {}", listener.local_addr().unwrap());
        axum::serve(listener, self.router).await?;
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}
