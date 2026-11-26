//! The tokens of upstreams that require authentication.
//!
//! An upstream that asks for a token asks for it in its challenge, and the token it returns
//! serves **several** requests: asking for it again for each manifest is
//! an extra request for every image pulled.
//!
//! The token is a credential, so the cache is **per upstream**: two
//! repositories that point to the same place but with different credentials do not
//! share a token, because they are not the same identity. The key is set by
//! the caller, and the plane knows nothing about repositories.
//!
//! And the token expires: what the upstream itself says governs, with a
//! cap so that an upstream saying "lasts a day" does not leave us a token
//! alive for a day. An expired token is requested again, which is what a real
//! client does.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The cap on how long a token lives in the cache.
pub const MAX_TOKEN_TTL: Duration = Duration::from_secs(300);

/// A token and when it expires.
#[derive(Debug, Clone)]
struct TheToken {
    /// The token.
    token: String,
    /// When it stops being valid.
    expires_at: Instant,
}

/// The tokens, per upstream.
#[derive(Debug, Default)]
pub struct TokenCache {
    entries: Mutex<HashMap<String, TheToken>>,
}

impl TokenCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The token of an upstream, if there is one and it is still valid.
    pub fn get(&self, key: &str) -> Option<String> {
        let mut entries = self.entries.lock().ok()?;
        let token = entries.get(key)?;
        if token.expires_at <= Instant::now() {
            entries.remove(key);
            return None;
        }
        Some(token.token.clone())
    }

    /// A token is stored, with the lifetime the upstream states.
    ///
    /// The lifetime is capped: an upstream that declares a token for a
    /// day does not decide how long a credential lives on this machine.
    pub fn put(&self, key: &str, token: String, ttl: Duration) {
        let ttl = ttl.min(MAX_TOKEN_TTL);
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        entries.insert(
            key.to_string(),
            TheToken {
                token,
                expires_at: Instant::now() + ttl,
            },
        );
    }

    /// A token is forgotten: the upstream has rejected it.
    pub fn forget(&self, key: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(key);
        }
    }

    /// How many tokens there are.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_lives_until_it_expires() {
        let cache = TokenCache::new();
        assert!(cache.get("upstream").is_none());
        cache.put("upstream", "el-token".to_string(), Duration::from_secs(60));
        assert_eq!(cache.get("upstream").as_deref(), Some("el-token"));
        cache.put("upstream", "otro".to_string(), Duration::from_secs(0));
        assert_eq!(cache.get("upstream"), None);
    }

    #[test]
    fn the_life_of_a_token_is_bounded() {
        let cache = TokenCache::new();
        cache.put(
            "upstream",
            "el-token".to_string(),
            Duration::from_secs(86_400),
        );
        let entries = cache.entries.lock().expect("the lock");
        let token = entries.get("upstream").expect("the token");
        assert!(token.expires_at <= Instant::now() + MAX_TOKEN_TTL);
    }

    #[test]
    fn two_upstreams_do_not_share_a_token() {
        let cache = TokenCache::new();
        cache.put("uno", "a".to_string(), Duration::from_secs(60));
        cache.put("otro", "b".to_string(), Duration::from_secs(60));
        assert_eq!(cache.get("uno").as_deref(), Some("a"));
        assert_eq!(cache.get("otro").as_deref(), Some("b"));
        cache.forget("uno");
        assert!(cache.get("uno").is_none());
        assert_eq!(cache.get("otro").as_deref(), Some("b"));
    }
}
