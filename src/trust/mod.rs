//! La admision al canal trusted: que digest puede servirse.
//!
//! Cedar decide **quien** puede leer un repositorio; esto decide **que
//! software** se le sirve. Son dos obligaciones separadas y se piden las
//! dos: un permiso de Cedar no sirve un digest sin concesion, y una
//! concesion no sirve a quien Cedar no deja leer.
//!
//! La admision es un unico punto, usado por todas las rutas de lectura
//! (raw y OCI). Que cada ruta tenga su propia comprobacion es como se
//! acaba teniendo una ruta que se olvida: aqui no hay otra forma de servir
//! un digest de un repositorio con canal trusted.
//!
//! # Reglas
//!
//! - El canal es del repositorio ([`ServeChannel`]); el `open` es el de
//!   cuarentena y sirve lo publicado.
//! - En el canal `trusted` solo se sirve un **digest** con una concesion
//!   activa. El nombre no cuenta: una etiqueta que se mueve apunta a otro
//!   digest, y ese no hereda nada.
//! - Lo que no se admite responde como **inexistente**, no como
//!   prohibido: un 403 diria que el contenido existe.
//! - No hay cache. Cada peticion lee el libro, de modo que una revocacion
//!   se ve en la peticion siguiente.
//! - Un libro que falla, o que no esta, **no** es un permiso: el canal
//!   trusted sin libro responde "no disponible".

pub mod hrn;
pub mod promotion;

use std::collections::HashSet;

use hodei_catalog::{RepositoryDescriptor, ServeChannel};
use hodei_evidence::ContentDigest;

use crate::app_state::AppState;

/// Por que no se sirve algo del canal trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// El digest no tiene concesion activa: para quien lee, no existe.
    Hidden,
    /// No se pudo saber: el libro falla o no esta. Nunca se sirve.
    Unavailable,
}

/// Lo que dijo el libro, sin interpretar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// Hay una concesion activa.
    Granted,
    /// No la hay.
    NotGranted,
    /// El libro fallo.
    Failed,
    /// La instancia no tiene libro.
    NoLedger,
}

/// El veredicto: funcion pura de canal y respuesta del libro.
///
/// Todo lo que no es un `Granted` explicito en el canal trusted es una
/// negativa.
pub fn verdict(channel: ServeChannel, lookup: Lookup) -> Result<(), Admission> {
    match (channel, lookup) {
        (ServeChannel::Open, _) => Ok(()),
        (ServeChannel::TrustedOnly, Lookup::Granted) => Ok(()),
        (ServeChannel::TrustedOnly, Lookup::NotGranted) => Err(Admission::Hidden),
        (ServeChannel::TrustedOnly, Lookup::Failed | Lookup::NoLedger) => {
            Err(Admission::Unavailable)
        }
    }
}

/// Si ese digest del repositorio puede servirse.
pub async fn admit(
    state: &AppState,
    account: &str,
    repository: &RepositoryDescriptor,
    digest: &ContentDigest,
) -> Result<(), Admission> {
    let channel = ServeChannel::of(&repository.config);
    if channel == ServeChannel::Open {
        return Ok(());
    }
    let lookup = match state.trust_ledger.as_ref() {
        None => Lookup::NoLedger,
        Some(ledger) => match ledger
            .active_grant(account, &repository.repo_key, digest)
            .await
        {
            Ok(Some(grant)) if grant.is_active() => Lookup::Granted,
            Ok(_) => Lookup::NotGranted,
            Err(_) => Lookup::Failed,
        },
    };
    verdict(channel, lookup)
}

/// Los digests servibles de un repositorio, para filtrar listados.
///
/// `Ok(None)` es el canal abierto: no hay nada que filtrar. `Ok(Some(..))`
/// es el conjunto de digests con concesion activa, en hexadecimal.
pub async fn servable(
    state: &AppState,
    account: &str,
    repository: &RepositoryDescriptor,
) -> Result<Option<HashSet<String>>, Admission> {
    if ServeChannel::of(&repository.config) == ServeChannel::Open {
        return Ok(None);
    }
    let ledger = state.trust_ledger.as_ref().ok_or(Admission::Unavailable)?;
    let grants = ledger
        .grants_of(account, &repository.repo_key)
        .await
        .map_err(|_| Admission::Unavailable)?;
    Ok(Some(
        grants
            .into_iter()
            .filter(|grant| grant.is_active())
            .map(|grant| grant.digest.value)
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_channel_serves_whatever_the_ledger_says() {
        for lookup in [
            Lookup::Granted,
            Lookup::NotGranted,
            Lookup::Failed,
            Lookup::NoLedger,
        ] {
            assert_eq!(verdict(ServeChannel::Open, lookup), Ok(()));
        }
    }

    #[test]
    fn the_trusted_channel_serves_only_an_explicit_grant() {
        assert_eq!(verdict(ServeChannel::TrustedOnly, Lookup::Granted), Ok(()));
        assert_eq!(
            verdict(ServeChannel::TrustedOnly, Lookup::NotGranted),
            Err(Admission::Hidden)
        );
    }

    #[test]
    fn an_unknown_state_is_never_a_permission() {
        for lookup in [Lookup::Failed, Lookup::NoLedger] {
            assert_eq!(
                verdict(ServeChannel::TrustedOnly, lookup),
                Err(Admission::Unavailable),
                "{lookup:?}"
            );
        }
    }
}
