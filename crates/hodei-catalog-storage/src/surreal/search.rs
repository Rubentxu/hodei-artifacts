//! La busqueda del catalogo sobre la base.
//!
//! No hay motor de busqueda: las filas de `catalog_asset` son lo que se
//! busca, y los indices de la base son lo que las hace encontrables. Un
//! indice aparte obligaria a copiar el catalogo en otra parte, y una copia
//! es donde un indice empieza a mentir.
//!
//! # Las tres consultas
//!
//! Una busqueda son tres preguntas a la base:
//!
//! 1. **Cuantos hay.** El total, con la misma condicion que la pagina:
//!    un total que no cuenta lo mismo que la pagina es un total que
//!    miente.
//! 2. **Cuales son.** La pagina, con el orden `(repositorio, ruta)`, que
//!    es un orden total dentro de la cuenta.
//! 3. **En que canal estan.** El estado de confianza es de la
//!    *relacion* entre el repositorio y el contenido, y vive en
//!    `catalog_reference`: no se puede leer en la misma consulta sin una
//!    union que el motor no tiene, asi que se pregunta aparte por los
//!    assets de la pagina.
//!
//! # El `IN` sobre arrays no se usa
//!
//! En SurrealDB 2.7 con RocksDB, un filtro `campo IN <array>` en la
//! consulta exterior devuelve **cero filas de forma intermitente**: se
//! midio con 100 rondas entrelazadas sobre la misma base, y 28 de ellas
//! vinieron vacias. La misma consulta con un `OR` de igualdades, o con un
//! `IN` sobre una subconsulta, no fallo ninguna vez.
//!
//! Por eso los repositorios permitidos entran por una subconsulta y los
//! canales de la pagina por un `OR` de pares (repositorio, ruta). La
//! segunda forma tiene una ventaja mas: cada termino es una busqueda
//! exacta en el indice `(account, repository, asset_path)`.
//!
//! Y el `LIMIT` y el `START` van como numeros, no como parametros: el
//! motor no sabe evaluar un `LIMIT` atado (`Found NONE but the LIMIT
//! clause must evaluate to a positive integer`). Los dos numeros salen de
//! un `usize` validado, asi que no hay texto del cliente pegado en la
//! consulta.

use std::collections::HashMap;

use async_trait::async_trait;
use hodei_catalog::search::{RepositoryAttribute, SearchHit, SearchPage, SearchQuery};
use hodei_catalog::{
    CatalogError, CatalogSearch, RepositoryDescriptor, RepositoryFormat, TrustState,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{AssetRow, RepositoryRow, SurrealCatalogStore, algorithm_of};

/// Cuantos repositorios pueden compartir una etiqueta.
///
/// Una etiqueta vive en un repositorio, y el mismo nombre puede estar en
/// muchos. El limite es una frontera: una busqueda con mil repositorios
/// con la misma etiqueta no es una busqueda.
const MAX_TAG_TARGETS: usize = 64;

/// La fila de una referencia, con su canal.
#[derive(Debug, Clone, Deserialize)]
struct ChannelRow {
    repository: String,
    asset_path: String,
    trust_state: String,
}

/// Las coordenadas a las que apunta una etiqueta.
#[derive(Debug, Clone, Deserialize)]
struct TagRow {
    repository: String,
    package_name: String,
    package_version: String,
}

/// El total de una consulta.
#[derive(Debug, Clone, Deserialize)]
struct CountRow {
    total: i64,
}

#[async_trait]
impl CatalogSearch for SurrealCatalogStore {
    async fn repositories(
        &self,
        principal_account: &str,
    ) -> Result<Vec<RepositoryDescriptor>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT account, repo_key, hrn, repo_format, repo_mode, config_version, \
                 digest_scope, config_settings, created_at, updated_at FROM catalog_repository \
                 WHERE account = $account ORDER BY repo_key",
            )
            .bind(("account", principal_account.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<RepositoryRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        rows.into_iter()
            .map(RepositoryRow::into_descriptor)
            .collect()
    }

    async fn search(
        &self,
        principal_account: &str,
        repositories: &[RepositoryDescriptor],
        query: &SearchQuery,
    ) -> Result<SearchPage, CatalogError> {
        query
            .validate()
            .map_err(|error| CatalogError::invalid(error.to_string()))?;

        // Un principal que no puede leer ningun repositorio no ve nada: la
        // busqueda no es un camino mas alla del permiso.
        if repositories.is_empty() {
            return Ok(SearchPage::empty(query));
        }
        let permitted: Vec<String> = repositories
            .iter()
            .map(|repository| repository.repo_key.clone())
            .collect();
        let formats: HashMap<String, RepositoryFormat> = repositories
            .iter()
            .map(|repository| (repository.repo_key.clone(), repository.format))
            .collect();

        // La etiqueta se resuelve antes: lo que acota la busqueda son sus
        // coordenadas, no su nombre.
        let targets = match &query.tag {
            Some(tag) => self.the_tag_targets(principal_account, tag).await?,
            None => Vec::new(),
        };
        if query.tag.is_some() && targets.is_empty() {
            // Una etiqueta que no existe no es una busqueda sin
            // resultados: es una busqueda de una etiqueta que no esta.
            return Ok(SearchPage::empty(query));
        }

        let (conditions, bindings) = the_conditions(principal_account, &permitted, query, &targets);
        let where_clause = conditions.join(" AND ");

        // El total. La misma condicion que la pagina: un total que no
        // cuenta lo mismo que la pagina es un total que miente.
        let count_sql =
            format!("SELECT count() AS total FROM catalog_asset WHERE {where_clause} GROUP ALL");
        let mut response = self
            .db
            .query(&count_sql)
            .bind(bindings.clone())
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let totals: Vec<CountRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let total = totals.first().map(|row| row.total as usize).unwrap_or(0);

        // La pagina. El limite y el arranque van como numeros: el motor no
        // sabe evaluar un `LIMIT` atado, y los dos son un `usize` que no
        // puede traer texto del cliente.
        let page_sql = format!(
            "SELECT account, repository, asset_path, package_name, package_version, \
             digest_algorithm, digest, size_bytes, subject, updated_at FROM catalog_asset \
             WHERE {where_clause} ORDER BY repository, asset_path LIMIT {} START {}",
            query.limit, query.offset
        );
        let mut response = self
            .db
            .query(&page_sql)
            .bind(bindings)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<AssetRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;

        // El canal de cada asset de la pagina.
        let channels = self.the_channels(principal_account, &rows).await?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let asset = row.into_asset()?;
            let format = formats
                .get(&asset.repository)
                .copied()
                .unwrap_or(RepositoryFormat::Raw);
            let trust_state = channels
                .get(&(asset.repository.clone(), asset.asset_path.clone()))
                .copied()
                .unwrap_or(TrustState::Quarantined);
            results.push(SearchHit {
                repository: asset.repository,
                format,
                asset_path: asset.asset_path,
                package_name: asset.coordinates.package_name,
                package_version: asset.coordinates.package_version,
                digest: asset.digest,
                size_bytes: asset.size_bytes,
                trust_state,
                updated_at: asset.updated_at,
            });
        }

        Ok(SearchPage {
            total,
            limit: query.limit,
            offset: query.offset,
            results,
        })
    }
}

impl SurrealCatalogStore {
    /// Las coordenadas a las que apunta una etiqueta de la cuenta.
    async fn the_tag_targets(
        &self,
        principal_account: &str,
        tag: &str,
    ) -> Result<Vec<TagRow>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT repository, package_name, package_version FROM catalog_tag \
                 WHERE account = $account AND tag_name = $tag",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("tag", tag.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<TagRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        if rows.len() > MAX_TAG_TARGETS {
            return Err(CatalogError::invalid(
                "too many repositories carry that tag to search by it",
            ));
        }
        Ok(rows)
    }

    /// El canal de los assets de la pagina.
    ///
    /// La pregunta es por pares (repositorio, ruta), y cada par es una
    /// busqueda exacta en el indice. La forma con un `IN` sobre un array
    /// seria mas corta, pero el motor devuelve a veces cero filas con esa
    /// forma, y un canal que no se lee es un canal que no se sabe.
    async fn the_channels(
        &self,
        principal_account: &str,
        rows: &[AssetRow],
    ) -> Result<HashMap<(String, String), TrustState>, CatalogError> {
        if rows.is_empty() {
            return Ok(HashMap::new());
        }
        let mut conditions = vec!["account = $account".to_string()];
        let mut bindings = Map::new();
        bindings.insert("account".to_string(), json!(principal_account));
        for (index, row) in rows.iter().enumerate() {
            conditions.push(format!(
                "(repository = $channel_repository_{index} AND asset_path = $channel_path_{index})"
            ));
            bindings.insert(format!("channel_repository_{index}"), json!(row.repository));
            bindings.insert(format!("channel_path_{index}"), json!(row.asset_path));
        }
        let sql = format!(
            "SELECT repository, asset_path, trust_state FROM catalog_reference WHERE {}",
            conditions.join(" AND ")
        );
        let mut response = self
            .db
            .query(&sql)
            .bind(bindings)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<ChannelRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let state = TrustState::of(&row.trust_state);
                ((row.repository, row.asset_path), state)
            })
            .collect())
    }
}

/// La condicion de la busqueda, con lo que lleva atado.
///
/// Los filtros van atados a parametros: el texto que el cliente escribe
/// nunca se pega en la consulta.
fn the_conditions(
    principal_account: &str,
    permitted: &[String],
    query: &SearchQuery,
    targets: &[TagRow],
) -> (Vec<String>, Map<String, Value>) {
    let mut conditions = vec![
        "account = $account".to_string(),
        // Los repositorios permitidos entran por una subconsulta: un `IN`
        // sobre un array en el filtro exterior devuelve a veces cero
        // filas, y una busqueda que a veces no ve nada no es una
        // busqueda.
        "repository IN (SELECT VALUE repo_key FROM catalog_repository \
         WHERE account = $account AND repo_key IN $repositories)"
            .to_string(),
    ];
    let mut bindings = Map::new();
    bindings.insert("account".to_string(), json!(principal_account));
    bindings.insert("repositories".to_string(), json!(permitted));

    if let Some(repository) = &query.repository {
        conditions.push("repository = $repository".to_string());
        bindings.insert("repository".to_string(), json!(repository));
    }
    if let Some(package_name) = &query.package_name {
        conditions.push("package_name = $package_name".to_string());
        bindings.insert("package_name".to_string(), json!(package_name));
    }
    if let Some(package_version) = &query.package_version {
        conditions.push("package_version = $package_version".to_string());
        bindings.insert("package_version".to_string(), json!(package_version));
    }
    if let Some(digest) = &query.digest {
        conditions.push("digest_algorithm = $digest_algorithm".to_string());
        conditions.push("digest = $digest".to_string());
        bindings.insert("digest_algorithm".to_string(), json!(algorithm_of(digest)));
        bindings.insert("digest".to_string(), json!(digest.value));
    }
    if !targets.is_empty() {
        // La etiqueta acota por coordenadas: un asset que no es el de la
        // etiqueta no sale, aunque este en el mismo repositorio.
        let mut groups = Vec::new();
        for (index, target) in targets.iter().enumerate() {
            groups.push(format!(
                "(repository = $tag_repository_{index} AND package_name = $tag_package_{index} \
                 AND package_version = $tag_version_{index})"
            ));
            bindings.insert(format!("tag_repository_{index}"), json!(target.repository));
            bindings.insert(format!("tag_package_{index}"), json!(target.package_name));
            bindings.insert(
                format!("tag_version_{index}"),
                json!(target.package_version),
            );
        }
        conditions.push(format!("({})", groups.join(" OR ")));
    }
    if let Some(attribute) = &query.attribute {
        let (condition, binding) = the_attribute_condition(attribute);
        conditions.push(condition);
        if let Some(binding) = binding {
            bindings.insert("attribute".to_string(), binding);
        }
    }
    if let Some(text) = &query.text {
        // El texto se busca en el nombre del paquete y en la ruta, sin
        // distinguir mayusculas. No hay indice de texto completo: esto es
        // un recorrido de los assets de la cuenta.
        conditions.push(
            "(string::contains(string::lowercase(package_name), string::lowercase($text)) \
             OR string::contains(string::lowercase(asset_path), string::lowercase($text)))"
                .to_string(),
        );
        bindings.insert("text".to_string(), json!(text));
    }
    (conditions, bindings)
}

/// La condicion de un atributo de repositorio.
///
/// Los atributos son los que el catalogo guarda: el formato, el modo y el
/// canal. El canal se lee como lo lee el resto del servicio: un
/// repositorio sin ajustes de confianza es abierto, y uno que los tiene
/// pero no se entienden es de confianza.
fn the_attribute_condition(attribute: &RepositoryAttribute) -> (String, Option<Value>) {
    match attribute {
        RepositoryAttribute::Format(_) => (
            "repository IN (SELECT VALUE repo_key FROM catalog_repository \
             WHERE account = $account AND repo_format = $attribute)"
                .to_string(),
            Some(json!(attribute.value())),
        ),
        RepositoryAttribute::Mode(_) => (
            "repository IN (SELECT VALUE repo_key FROM catalog_repository \
             WHERE account = $account AND repo_mode = $attribute)"
                .to_string(),
            Some(json!(attribute.value())),
        ),
        RepositoryAttribute::Channel(channel) => match channel {
            hodei_catalog::ServeChannel::Open => (
                "repository IN (SELECT VALUE repo_key FROM catalog_repository \
                 WHERE account = $account AND (config_settings['trust'] = NONE \
                 OR config_settings['trust'].serve = 'open'))"
                    .to_string(),
                None,
            ),
            hodei_catalog::ServeChannel::TrustedOnly => (
                "repository IN (SELECT VALUE repo_key FROM catalog_repository \
                 WHERE account = $account AND config_settings['trust'] != NONE \
                 AND config_settings['trust'].serve != 'open')"
                    .to_string(),
                None,
            ),
        },
    }
}
