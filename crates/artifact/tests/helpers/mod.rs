use tracing_subscriber::FmtSubscriber;
use tracing::Level;
use std::sync::Once;

static INIT: Once = Once::new();

pub fn setup_tracing() {
    INIT.call_once(|| {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed");
    });
}