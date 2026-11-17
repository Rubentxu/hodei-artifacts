//! La importacion del paquete: resolver sin salir a la red.
//!
//! Importar es la operacion inversa y asimetrica. Asimetrica porque el
//! origen del paquete es **procedencia, no autoridad**: lo que autoriza a
//! servir un contenido en el destino es la configuracion del destino, y por
//! eso el recibo que se escribe aqui se firma con el upstream y la version
//! de configuracion vigentes, con el resultado `imported`, mientras la URL
//! y la decision originales se conservan solo como evidencia dentro del
//! manifiesto.
//!
//! La visibilidad se gana al reves de como se cuenta: **nada queda
//! publicable hasta que el paquete entero se ha verificado**. Primero se
//! comprueban y se almacenan los blobs contra el digito por digito del
//! manifiesto; solo despues, y en un orden que no deja rutas huérfanas, se
//! escriben los recibos y se publican las rutas. Un manifiesto mentiroso o
//! un fichero corrupto abortan la importacion sin hacer visible nada.

use std::path::Path;

use hodei_catalog::{
    CatalogReader, CatalogWriter, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
    RepositoryMode, UpstreamConfig,
};
use hodei_hub::BlobStore;

use super::manifest::{BundleManifest, MANIFEST_NAME};
use super::{BundleError, content_digest};

/// Lo que hizo una importacion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportReport {
    /// Las rutas publicadas por esta importacion.
    pub imported: usize,
    /// Las rutas que ya estaban, con el mismo contenido.
    pub already_present: usize,
    /// Los bytes de blobs que llegaron al almacen del destino.
    pub blob_bytes: u64,
}

/// Como importar un paquete en un destino que no es el del origen.
#[derive(Debug, Clone, Copy, Default)]
pub struct Mapping {
    /// Permitir importar en una cuenta o un repositorio distintos del
    /// origen. Sin esto, la importacion cruzada se rechaza: el que importa
    /// tiene que decir explicitamente que esa ruta le pertenece.
    pub allow: bool,
    /// Permitir que una ruta del paquete sustituya otra en el destino.
    /// Sin esto, el choque de rutas aborta la importacion.
    pub overwrite: bool,
}

/// Lee y comprueba el manifiesto de un paquete, sin tocar el catalogo.
pub fn read_manifest(directory: &Path) -> Result<BundleManifest, BundleError> {
    let raw = std::fs::read(directory.join(MANIFEST_NAME)).map_err(|error| {
        BundleError::Read(format!(
            "the bundle manifest at {} cannot be read: {error}",
            directory.display()
        ))
    })?;
    let manifest: BundleManifest = serde_json::from_slice(&raw)?;
    manifest.check_importable()?;
    Ok(manifest)
}

/// Importa el paquete de `directory` en el repositorio proxy `repository`
/// de `account`.
#[allow(clippy::too_many_arguments)]
pub async fn import(
    reader: &dyn CatalogReader,
    writer: &dyn CatalogWriter,
    ledger: &dyn ProvenanceLedger,
    blobs: &dyn BlobStore,
    account: &str,
    repository: &str,
    directory: &Path,
    mapping: Mapping,
    actor: &str,
    correlation_id: &str,
) -> Result<ImportReport, BundleError> {
    let manifest = read_manifest(directory)?;
    let descriptor = reader
        .repository(account, repository)
        .await
        .map_err(|error| {
            BundleError::Refused(format!(
                "the destination repository {account}/{repository} does not answer: {error}; \
             create the proxy repository before importing"
            ))
        })?;
    if descriptor.mode != RepositoryMode::Proxy {
        return Err(BundleError::Refused(format!(
            "the destination is a {}, and a resolution bundle only feeds a proxy",
            descriptor.mode
        )));
    }
    let upstream = UpstreamConfig::of(&descriptor.config)
        .map_err(|error| {
            BundleError::Refused(format!("the destination configuration is broken: {error}"))
        })?
        .ok_or_else(|| {
            BundleError::Refused("the destination proxy has no upstream configured".to_string())
        })?;
    if upstream.id != manifest.origin.upstream_id {
        return Err(BundleError::Refused(format!(
            "the destination resolves against {} and the bundle was resolved against {}; \
             provenance is not authority — point the proxy at the same upstream or refuse \
             the package",
            upstream.id, manifest.origin.upstream_id
        )));
    }
    if !mapping.allow
        && (manifest.origin.account != account || manifest.origin.repository != repository)
    {
        return Err(BundleError::Refused(format!(
            "the bundle comes from {}/{}, another owner than {}/{}, and importing across \
             owners needs the explicit mapping flag",
            manifest.origin.account, manifest.origin.repository, account, repository
        )));
    }

    // 1. Verificar y almacenar los blobs. Nada del catalogo se toca aqui:
    //    un paquete podrido aborta antes de que ninguna ruta sea visible.
    let mut blob_bytes = 0;
    for blob in &manifest.blobs {
        let digest = content_digest(&blob.digest).expect("a checked manifest has real digests");
        let path = directory.join(BundleManifest::blob_file(&blob.digest));
        let bytes = std::fs::read(&path).map_err(|error| {
            BundleError::Unsound(format!(
                "the blob {} of the package is not at {}: {error}",
                blob.digest,
                path.display()
            ))
        })?;
        let computed = hodei_evidence::ContentDigest::of_bytes(&bytes);
        if computed != digest {
            return Err(BundleError::Unsound(format!(
                "the file {} is not the content the manifest declares: its digest is {computed}",
                path.display()
            )));
        }
        if bytes.len() as u64 != blob.length {
            return Err(BundleError::Unsound(format!(
                "the blob {} declares {} bytes and the file has {}",
                blob.digest,
                blob.length,
                bytes.len()
            )));
        }
        blobs.put(&bytes).await.map_err(|error| {
            BundleError::Io(format!(
                "the blob {} cannot be stored: {error}",
                blob.digest
            ))
        })?;
        blob_bytes += blob.length;
    }

    // 2. Hacer visibles las rutas: primero el recibo que las respalda,
    //    despues la publicacion. Un corte entre las dos deja respaldo sin
    //    ruta, nunca una ruta sin respaldo.
    let mut imported = 0;
    let mut already_present = 0;
    for entry in &manifest.assets {
        let digest = content_digest(&entry.digest).expect("a checked manifest has real digests");
        ledger
            .record(&ProvenanceReceipt {
                account: account.to_string(),
                repository: repository.to_string(),
                upstream_id: upstream.id.clone(),
                config_version: descriptor.config.version,
                outcome: ReceiptOutcome::Imported,
                actor: actor.to_string(),
                correlation_id: correlation_id.to_string(),
                ..entry.receipt.clone()
            })
            .await?;

        let existing = reader.asset(account, repository, &entry.asset_path).await;
        if let Ok(asset) = &existing
            && asset.digest == digest
        {
            already_present += 1;
            continue;
        }
        if existing.is_ok() && !mapping.overwrite {
            return Err(BundleError::Refused(format!(
                "the path {} points to other content in the destination; importing over it \
                 needs the explicit overwrite flag",
                entry.asset_path
            )));
        }
        writer
            .publish(
                account,
                hodei_catalog::Publication {
                    repository: repository.to_string(),
                    asset_path: entry.asset_path.clone(),
                    coordinates: hodei_catalog::PackageCoordinates {
                        package_name: entry.package.clone(),
                        package_version: entry.version.clone(),
                    },
                    digest,
                    size_bytes: entry.size_bytes,
                    tags: Vec::new(),
                    actor: actor.to_string(),
                    correlation_id: correlation_id.to_string(),
                    overwrite: mapping.overwrite,
                    precondition: None,
                    subject: None,
                },
            )
            .await?;
        imported += 1;
    }

    Ok(ImportReport {
        imported,
        already_present,
        blob_bytes,
    })
}
