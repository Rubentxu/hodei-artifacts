//! El libro de los recibos de procedencia sobre la base del catalogo.
//!
//! Una fila por recibo. El identificador de la fila sale del identificador
//! del recibo, de modo que repetir una escritura la deja como estaba y un
//! recibo nunca se reescribe: lo que el upstream dijo ese dia queda como
//! se escribio.

use async_trait::async_trait;
use hodei_catalog::{
    CatalogError, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
};
use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};

use super::{SurrealCatalogStore, composite_id, digest_of};

fn unavailable(error: impl ToString) -> CatalogError {
    CatalogError::unavailable(error.to_string())
}

#[derive(Debug, Clone, Serialize)]
struct NewRow {
    id: Thing,
    receipt_id: String,
    account: String,
    repository: String,
    asset_path: String,
    upstream_id: String,
    source_url: String,
    config_version: i64,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    checksum_kind: String,
    checksum_header: Option<String>,
    status: i64,
    redirects: i64,
    etag: Option<String>,
    last_modified: Option<String>,
    outcome: String,
    actor: String,
    correlation_id: String,
    fetched_at: Datetime,
}

impl NewRow {
    fn of(receipt: &ProvenanceReceipt) -> Self {
        let (kind, header) = match &receipt.checksum {
            ChecksumEvidence::Declared { header } => ("declared", Some(header.clone())),
            ChecksumEvidence::ComputedOnly => ("computed_only", None),
        };
        Self {
            id: Thing::from((
                "provenance_receipt",
                composite_id(&[&receipt.account, &receipt.id]).as_str(),
            )),
            receipt_id: receipt.id.clone(),
            account: receipt.account.clone(),
            repository: receipt.repository.clone(),
            asset_path: receipt.asset_path.clone(),
            upstream_id: receipt.upstream_id.clone(),
            source_url: receipt.source_url.clone(),
            config_version: receipt.config_version as i64,
            digest_algorithm: receipt.digest.algorithm.to_string(),
            digest: receipt.digest.value.clone(),
            size_bytes: receipt.size_bytes as i64,
            checksum_kind: kind.to_string(),
            checksum_header: header,
            status: i64::from(receipt.status),
            redirects: i64::from(receipt.redirects),
            etag: receipt.etag.clone(),
            last_modified: receipt.last_modified.clone(),
            outcome: receipt.outcome.as_str().to_string(),
            actor: receipt.actor.clone(),
            correlation_id: receipt.correlation_id.clone(),
            fetched_at: Datetime::from(receipt.fetched_at),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Row {
    receipt_id: String,
    account: String,
    repository: String,
    asset_path: String,
    upstream_id: String,
    source_url: String,
    config_version: i64,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    checksum_kind: String,
    checksum_header: Option<String>,
    status: i64,
    redirects: i64,
    etag: Option<String>,
    // Los recibos de antes de la version 8 no llevan estos dos campos.
    #[serde(default)]
    last_modified: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
    actor: String,
    correlation_id: String,
    fetched_at: Datetime,
}

impl Row {
    fn the_receipt(self) -> Result<ProvenanceReceipt, CatalogError> {
        let checksum = match (self.checksum_kind.as_str(), self.checksum_header) {
            ("declared", Some(header)) => ChecksumEvidence::Declared { header },
            ("computed_only", None) => ChecksumEvidence::ComputedOnly,
            // Un recibo que no se entiende no se reinterpreta: afirmaria
            // una comprobacion que nadie sabe si se hizo.
            _ => {
                return Err(CatalogError::unavailable(
                    "a provenance receipt carries a checksum evidence this release cannot read",
                ));
            }
        };
        // Sin desenlace es un recibo de antes de la cache: se trajo. Un
        // desenlace que esta build no conoce no se reinterpreta.
        let outcome = match self.outcome.as_deref() {
            None => ReceiptOutcome::Fetched,
            Some(raw) => ReceiptOutcome::parse(raw).ok_or_else(|| {
                CatalogError::unavailable(
                    "a provenance receipt carries an outcome this release cannot read",
                )
            })?,
        };
        Ok(ProvenanceReceipt {
            digest: digest_of(&self.digest_algorithm, &self.digest)?,
            id: self.receipt_id,
            account: self.account,
            repository: self.repository,
            asset_path: self.asset_path,
            upstream_id: self.upstream_id,
            source_url: self.source_url,
            config_version: u64::try_from(self.config_version).unwrap_or_default(),
            size_bytes: u64::try_from(self.size_bytes).unwrap_or_default(),
            checksum,
            status: u16::try_from(self.status).unwrap_or_default(),
            redirects: u8::try_from(self.redirects).unwrap_or_default(),
            etag: self.etag,
            last_modified: self.last_modified,
            outcome,
            actor: self.actor,
            correlation_id: self.correlation_id,
            fetched_at: self.fetched_at.0,
        })
    }
}

const COLUMNS: &str = "receipt_id, account, repository, asset_path, upstream_id, source_url, \
                       config_version, digest_algorithm, digest, size_bytes, checksum_kind, \
                       checksum_header, status, redirects, etag, last_modified, outcome, actor, \
                       correlation_id, fetched_at";

#[async_trait]
impl ProvenanceLedger for SurrealCatalogStore {
    async fn record(&self, receipt: &ProvenanceReceipt) -> Result<(), CatalogError> {
        // `INSERT IGNORE`: repetir el mismo recibo no lo reescribe.
        self.db
            .query("INSERT IGNORE INTO provenance_receipt $row")
            .bind(("row", NewRow::of(receipt)))
            .await
            .map_err(unavailable)?
            .check()
            .map_err(unavailable)?;
        Ok(())
    }

    async fn receipts_of(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Vec<ProvenanceReceipt>, CatalogError> {
        let mut response = self
            .db
            .query(format!(
                "SELECT {COLUMNS} FROM provenance_receipt \
                 WHERE account = $account AND repository = $repository \
                 AND asset_path = $path ORDER BY fetched_at DESC, receipt_id DESC"
            ))
            .bind(("account", account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("path", asset_path.to_string()))
            .await
            .map_err(unavailable)?;
        let rows: Vec<Row> = response.take(0).map_err(unavailable)?;
        rows.into_iter().map(Row::the_receipt).collect()
    }

    async fn latest_receipts(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
        limit: usize,
    ) -> Result<Vec<ProvenanceReceipt>, CatalogError> {
        let mut response = self
            .db
            .query(format!(
                "SELECT {COLUMNS} FROM provenance_receipt \
                 WHERE account = $account AND repository = $repository \
                 AND asset_path = $path ORDER BY fetched_at DESC, receipt_id DESC \
                 LIMIT $limit"
            ))
            .bind(("account", account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("path", asset_path.to_string()))
            .bind(("limit", i64::try_from(limit).unwrap_or(i64::MAX)))
            .await
            .map_err(unavailable)?;
        let rows: Vec<Row> = response.take(0).map_err(unavailable)?;
        rows.into_iter().map(Row::the_receipt).collect()
    }
}
