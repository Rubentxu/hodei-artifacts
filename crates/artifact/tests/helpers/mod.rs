use std::sync::Once;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

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
