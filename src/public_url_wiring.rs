//! The public address and the signed-URL ring, wired once for every profile.
//!
//! Both bootstrap profiles (embedded and graph) must give the application the
//! same public address and the same signing keys under the same startup
//! validation. They both call [`wire_public_url_and_signing`]: there is one
//! copy of the semantics, so the profiles cannot drift apart.

use tracing::{info, warn};

use crate::app_state::AppState;
use crate::bootstrap::BootstrapError;
use crate::config::AppConfig;

/// Install the configured public URL and the ring that signs the archive URLs
/// of the provider mirror.
///
/// The public address is decided by the configuration and never by the
/// request: a hostile `Host` or `X-Forwarded-Proto` must not change the URLs
/// the registry returns. A configuration that cannot be understood does not
/// boot. A signing key that is named and cannot be read stops the boot; no key
/// at all is allowed and is announced, because the mirror then refuses to
/// issue archive URLs and a Terraform client cannot install from it.
pub fn wire_public_url_and_signing(
    app_state: AppState,
    config: &AppConfig,
) -> Result<AppState, BootstrapError> {
    let app_state = app_state.with_public_url(
        config
            .public_url
            .the_public_url()
            .map_err(|error| BootstrapError::Initialization(error.to_string()))?,
    );
    let signed_urls = config
        .signing
        .the_ring()
        .map_err(|error| BootstrapError::Initialization(format!("signing keys: {error}")))?;
    if signed_urls.is_configured() {
        info!(
            ttl_secs = signed_urls.ttl_secs(),
            "the signed URLs of the provider mirror are enabled"
        );
    } else {
        warn!(
            "no signing key is configured: the Terraform provider mirror will not issue archive URLs and its version documents answer 503; add a key to [signing]"
        );
    }
    Ok(app_state.with_signed_urls(signed_urls))
}
