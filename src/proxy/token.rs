//! Los tokens de los upstreams que exigen autenticacion.
//!
//! Un upstream que pide un token lo pide en su reto, y el token que devuelve
//! sirve para **varias** peticiones: pedirlo otra vez por cada manifiesto es
//! una peticion de mas por cada imagen que se tira.
//!
//! El token es una credencial, asi que la cache es **por upstream**: dos
//! repositorios que apuntan al mismo sitio pero con credenciales distintas no
//! comparten token, porque no son la misma identidad. La clave la pone
//! quien llama, y el plano no sabe nada de repositorios.
//!
//! Y el token caduca: lo que dice el propio upstream es lo que manda, con un
//! tope para que un upstream que dice "dura un dia" no nos deje un token
//! vivo un dia. Un token caducado se pide otra vez, que es lo que hace un
//! cliente de verdad.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// El tope de cuanto vive un token en la cache.
pub const MAX_TOKEN_TTL: Duration = Duration::from_secs(300);

/// Un token y cuando caduca.
#[derive(Debug, Clone)]
struct TheToken {
    /// El token.
    token: String,
    /// Cuando deja de valer.
    expires_at: Instant,
}

/// Los tokens, por upstream.
#[derive(Debug, Default)]
pub struct TokenCache {
    entries: Mutex<HashMap<String, TheToken>>,
}

impl TokenCache {
    /// Una cache vacia.
    pub fn new() -> Self {
        Self::default()
    }

    /// El token de un upstream, si lo hay y sigue valiendo.
    pub fn get(&self, key: &str) -> Option<String> {
        let mut entries = self.entries.lock().ok()?;
        let token = entries.get(key)?;
        if token.expires_at <= Instant::now() {
            entries.remove(key);
            return None;
        }
        Some(token.token.clone())
    }

    /// Se guarda un token, con el tiempo de vida que dice el upstream.
    ///
    /// El tiempo de vida se acota: un upstream que declara un token para un
    /// dia no decide cuanto vive una credencial en esta maquina.
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

    /// Se olvida un token: el upstream lo ha rechazado.
    pub fn forget(&self, key: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(key);
        }
    }

    /// Cuantos tokens hay.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// Si no hay ninguno.
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
