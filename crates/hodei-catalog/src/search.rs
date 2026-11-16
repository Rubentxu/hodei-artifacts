//! La busqueda del catalogo.
//!
//! La busqueda no tiene motor: el indice es el catalogo. Las filas de
//! `catalog_asset` son lo que se busca, y los indices de la base son lo que
//! las hace encontrables. Un segundo indice obligaria a sincronizarlo con el
//! catalogo en cada escritura, y el sincronismo es donde un indice se
//! convierte en una copia que miente.
//!
//! # Lo que se busca
//!
//! Un asset, con la forma de su repositorio y el estado de su referencia:
//!
//! - el repositorio y su formato, que es lo que le da sentido al asset;
//! - la ruta, que es lo que el cliente pide;
//! - el nombre y la version del paquete, que son sus coordenadas;
//! - el digest y el tamano, que son el contenido;
//! - el estado de confianza, que es de la **relacion** entre el
//!   repositorio y el contenido, no del contenido.
//!
//! # El orden
//!
//! El orden es `(repositorio, ruta)`. Es un orden **total** dentro de la
//! cuenta: dos assets distintos no pueden compartir las dos claves, asi que
//! la pagina no puede repetir ni perder un resultado entre paginas, y el
//! mismo filtro devuelve la misma pagina en el mismo orden.
//!
//! # La cuenta
//!
//! La cuenta no es un filtro que se pueda olvidar: es parte de la
//! consulta. Una busqueda es de una cuenta, y los resultados de otra no
//! existen para ella.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use hodei_evidence::domain::content::DigestAlgorithm;
use serde::{Deserialize, Serialize};

use crate::model::{RepositoryFormat, RepositoryMode, TrustState};
use crate::trust::ServeChannel;

/// Cuantos resultados trae una pagina, como maximo.
///
/// Un limite alto es un limite de memoria y de tiempo de la base, y un
/// limite bajo hace que la busqueda no sirva para lo que se busca. El
/// cliente puede pedir menos.
pub const MAX_LIMIT: usize = 100;

/// Cuantos resultados trae una pagina si el cliente no dice.
pub const DEFAULT_LIMIT: usize = 20;

/// Cuantos resultados se pueden dejar atras.
///
/// Un desplazamiento enorme no es una pagina: es una pregunta que no
/// tiene respuesta util, y el motor tiene que recorrerla entera para
/// saber que no hay nada.
pub const MAX_OFFSET: usize = 100_000;

/// El error de una consulta de busqueda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// El limite de la pagina no cabe.
    BadLimit(usize),
    /// El desplazamiento no cabe.
    BadOffset(usize),
    /// Un filtro esta vacio.
    EmptyFilter(&'static str),
    /// El digest no se puede leer.
    BadDigest(String),
    /// El atributo no se puede leer.
    BadAttribute(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadLimit(limit) => {
                write!(formatter, "the limit {limit} is not a page size")
            }
            Self::BadOffset(offset) => {
                write!(formatter, "the offset {offset} is beyond the window")
            }
            Self::EmptyFilter(name) => write!(formatter, "the filter {name} is empty"),
            Self::BadDigest(value) => write!(formatter, "the digest {value} is not a digest"),
            Self::BadAttribute(value) => {
                write!(formatter, "the attribute {value} is not an attribute")
            }
        }
    }
}

impl std::error::Error for SearchError {}

/// Un atributo de un repositorio.
///
/// Son los que el catalogo guarda hoy: el formato, el modo y el canal por
/// el que se sirve. No hay atributos por asset, y esta enumeracion es
/// cerrada a proposito: un atributo que no existe no se puede buscar, y un
/// filtro que no dice nada no puede devolver lo que el cliente pretendia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryAttribute {
    /// El formato del contenido que guarda.
    Format(RepositoryFormat),
    /// Como se comporta respecto del exterior.
    Mode(RepositoryMode),
    /// El canal por el que se sirve su contenido.
    Channel(ServeChannel),
}

impl RepositoryAttribute {
    /// Lee un atributo de su texto: `nombre=valor`.
    ///
    /// El texto es el que escribe el cliente en la consulta, y el
    /// atributo que no se conoce no es un atributo: es una consulta que
    /// no dice nada.
    pub fn parse(text: &str) -> Result<Self, SearchError> {
        let (name, value) = text
            .split_once('=')
            .ok_or_else(|| SearchError::BadAttribute(text.to_string()))?;
        let value = value.trim();
        match name.trim() {
            "format" => Ok(Self::Format(
                RepositoryFormat::parse(value)
                    .ok_or_else(|| SearchError::BadAttribute(text.to_string()))?,
            )),
            "mode" => Ok(Self::Mode(
                RepositoryMode::parse(value)
                    .ok_or_else(|| SearchError::BadAttribute(text.to_string()))?,
            )),
            "channel" => Ok(Self::Channel(
                ServeChannel::parse(value)
                    .ok_or_else(|| SearchError::BadAttribute(text.to_string()))?,
            )),
            _ => Err(SearchError::BadAttribute(text.to_string())),
        }
    }

    /// El nombre del atributo, tal y como lo escribe el cliente.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Format(_) => "format",
            Self::Mode(_) => "mode",
            Self::Channel(_) => "channel",
        }
    }

    /// El valor del atributo, tal y como lo escribe el cliente.
    pub fn value(&self) -> &'static str {
        match self {
            Self::Format(format) => format.as_str(),
            Self::Mode(mode) => mode.as_str(),
            Self::Channel(channel) => channel.as_str(),
        }
    }
}

/// La consulta de una busqueda.
///
/// Los filtros son todos opcionales y se combinan con "y": un asset sale
/// cuando cumple todos los que estan puestos. Un filtro vacio es un
/// error y no un filtro que no acota nada: un `repository=` vacio no es
/// "todos los repositorios", es una consulta que no dice nada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// El repositorio, por su clave.
    pub repository: Option<String>,
    /// El nombre del paquete.
    pub package_name: Option<String>,
    /// La version del paquete.
    pub package_version: Option<String>,
    /// El digest del contenido.
    pub digest: Option<ContentDigest>,
    /// El nombre de una etiqueta.
    pub tag: Option<String>,
    /// Un atributo del repositorio.
    pub attribute: Option<RepositoryAttribute>,
    /// Un texto que aparece en el nombre del paquete o en la ruta.
    pub text: Option<String>,
    /// Cuantos resultados trae la pagina.
    pub limit: usize,
    /// Cuantos resultados se dejan atras.
    pub offset: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            repository: None,
            package_name: None,
            package_version: None,
            digest: None,
            tag: None,
            attribute: None,
            text: None,
            limit: DEFAULT_LIMIT,
            offset: 0,
        }
    }
}

impl SearchQuery {
    /// Una consulta sin filtros, con la pagina por defecto.
    pub fn new() -> Self {
        Self::default()
    }

    /// La consulta, con el filtro que se le pone.
    pub fn with_repository(mut self, repository: impl Into<String>) -> Self {
        self.repository = Some(repository.into());
        self
    }

    /// La consulta, con el filtro que se le pone.
    pub fn with_package_name(mut self, name: impl Into<String>) -> Self {
        self.package_name = Some(name.into());
        self
    }

    /// La consulta, con el filtro que se le pone.
    pub fn with_package_version(mut self, version: impl Into<String>) -> Self {
        self.package_version = Some(version.into());
        self
    }

    /// La consulta, con el filtro que se le pone.
    pub fn with_digest(mut self, digest: ContentDigest) -> Self {
        self.digest = Some(digest);
        self
    }

    /// La consulta, con el filtro de digest que escribe el cliente.
    ///
    /// El texto es `algoritmo:valor`, con el valor en hexadecimal. Un
    /// digest mal formado no es un filtro: es una consulta que no dice
    /// nada, y un filtro que no acota nada devolveria el catalogo
    /// entero.
    pub fn with_digest_text(mut self, text: &str) -> Result<Self, SearchError> {
        let (algorithm, value) = text
            .split_once(':')
            .ok_or_else(|| SearchError::BadDigest(text.to_string()))?;
        let algorithm = match algorithm.trim().to_ascii_lowercase().as_str() {
            "sha256" => DigestAlgorithm::Sha256,
            "sha512" => DigestAlgorithm::Sha512,
            _ => return Err(SearchError::BadDigest(text.to_string())),
        };
        let digest = ContentDigest {
            algorithm,
            value: value.trim().to_ascii_lowercase(),
        };
        if !digest.is_well_formed() {
            return Err(SearchError::BadDigest(text.to_string()));
        }
        self.digest = Some(digest);
        Ok(self)
    }

    /// La consulta, con el filtro que se le pone.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// La consulta, con el filtro que se le pone.
    pub fn with_attribute(mut self, attribute: RepositoryAttribute) -> Self {
        self.attribute = Some(attribute);
        self
    }

    /// La consulta, con el filtro que se le pone.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// La consulta, con el tamano de la pagina.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// La consulta, con lo que se deja atras.
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Si la consulta dice algo que buscar.
    ///
    /// Una consulta sin filtros es un recorrido de todo lo que hay, y eso
    /// no es una busqueda: es un volcado. Se puede pedir --el navegador de
    /// paquetes empieza por ahi--, pero quien lo pide lo sabe.
    pub fn has_a_filter(&self) -> bool {
        self.repository.is_some()
            || self.package_name.is_some()
            || self.package_version.is_some()
            || self.digest.is_some()
            || self.tag.is_some()
            || self.attribute.is_some()
            || self.text.is_some()
    }

    /// Comprueba que la consulta se puede hacer.
    ///
    /// Un filtro vacio es un error, el limite tiene que caber en una
    /// pagina, y el texto tiene que decir algo.
    pub fn validate(&self) -> Result<(), SearchError> {
        if self.limit == 0 || self.limit > MAX_LIMIT {
            return Err(SearchError::BadLimit(self.limit));
        }
        if self.offset > MAX_OFFSET {
            return Err(SearchError::BadOffset(self.offset));
        }
        for (value, name) in [
            (self.repository.as_deref(), "repository"),
            (self.package_name.as_deref(), "package"),
            (self.package_version.as_deref(), "version"),
            (self.tag.as_deref(), "tag"),
            (self.text.as_deref(), "text"),
        ] {
            if let Some(value) = value
                && value.trim().is_empty()
            {
                return Err(SearchError::EmptyFilter(name));
            }
        }
        Ok(())
    }
}

/// Un asset encontrado por una busqueda.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    /// El repositorio.
    pub repository: String,
    /// El formato del repositorio.
    pub format: RepositoryFormat,
    /// La ruta del asset dentro del repositorio.
    pub asset_path: String,
    /// El nombre del paquete.
    pub package_name: String,
    /// La version del paquete.
    pub package_version: String,
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que tiene el contenido.
    pub size_bytes: u64,
    /// El estado de confianza del contenido en este repositorio.
    ///
    /// Lo decide la **concesion**: lo que tiene una concesion vigente es
    /// de fiar, y lo que no la tiene esta en cuarentena. No se deduce de
    /// la ausencia de hallazgos: un contenido del que nadie ha escrito
    /// nada no es de fiar, es desconocido.
    pub trust_state: TrustState,
    /// Cuando se movio por ultima vez.
    pub updated_at: DateTime<Utc>,
}

/// Una pagina de resultados.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPage {
    /// Cuantos resultados hay en total.
    pub total: usize,
    /// Cuantos resultados trae la pagina.
    pub limit: usize,
    /// Cuantos resultados se dejan atras.
    pub offset: usize,
    /// Los resultados de la pagina.
    pub results: Vec<SearchHit>,
}

impl SearchPage {
    /// Una pagina vacia, con los numeros que se pidieron.
    pub fn empty(query: &SearchQuery) -> Self {
        Self {
            total: 0,
            limit: query.limit,
            offset: query.offset,
            results: Vec::new(),
        }
    }

    /// Si la pagina no trae nada.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
}

/// El orden de los resultados: repositorio y ruta.
///
/// Es un orden **total** dentro de la cuenta, y por eso la pagina no
/// repite ni pierde resultados entre paginas.
pub fn in_order(hits: &mut [SearchHit]) {
    hits.sort_by(|left, right| {
        left.repository
            .cmp(&right.repository)
            .then_with(|| left.asset_path.cmp(&right.asset_path))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_hit(repository: &str, asset_path: &str) -> SearchHit {
        SearchHit {
            repository: repository.to_string(),
            format: RepositoryFormat::PyPi,
            asset_path: asset_path.to_string(),
            package_name: "labpkg".to_string(),
            package_version: "1.0".to_string(),
            digest: ContentDigest::sha256("aa".repeat(32)),
            size_bytes: 10,
            trust_state: TrustState::Quarantined,
            updated_at: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    #[test]
    fn a_query_without_filters_is_a_walk() {
        assert!(!SearchQuery::new().has_a_filter());
        assert!(
            SearchQuery::new()
                .with_package_name("labpkg")
                .has_a_filter()
        );
        assert!(SearchQuery::new().validate().is_ok());
    }

    #[test]
    fn the_limit_of_a_page_is_bounded() {
        assert_eq!(SearchQuery::new().limit, DEFAULT_LIMIT);
        assert!(SearchQuery::new().with_limit(0).validate().is_err());
        assert!(SearchQuery::new().with_limit(MAX_LIMIT).validate().is_ok());
        assert!(
            SearchQuery::new()
                .with_limit(MAX_LIMIT + 1)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn the_offset_of_a_page_is_bounded() {
        assert!(SearchQuery::new().with_offset(0).validate().is_ok());
        assert!(
            SearchQuery::new()
                .with_offset(MAX_OFFSET)
                .validate()
                .is_ok()
        );
        assert!(
            SearchQuery::new()
                .with_offset(MAX_OFFSET + 1)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn an_empty_filter_is_not_a_filter() {
        assert!(SearchQuery::new().with_repository("  ").validate().is_err());
        assert!(SearchQuery::new().with_package_name("").validate().is_err());
        assert!(SearchQuery::new().with_tag("latest").validate().is_ok());
    }

    #[test]
    fn the_order_is_total_and_stable() {
        let mut hits = vec![a_hit("b", "two"), a_hit("a", "two"), a_hit("a", "one")];
        in_order(&mut hits);
        let order: Vec<(String, String)> = hits
            .iter()
            .map(|hit| (hit.repository.clone(), hit.asset_path.clone()))
            .collect();
        assert_eq!(
            order,
            vec![
                ("a".to_string(), "one".to_string()),
                ("a".to_string(), "two".to_string()),
                ("b".to_string(), "two".to_string()),
            ]
        );
    }

    #[test]
    fn an_empty_page_says_the_numbers_that_were_asked_for() {
        let query = SearchQuery::new().with_limit(7).with_offset(3);
        let page = SearchPage::empty(&query);
        assert_eq!(page.total, 0);
        assert_eq!(page.limit, 7);
        assert_eq!(page.offset, 3);
        assert!(page.is_empty());
    }

    #[test]
    fn a_digest_is_read_from_its_text() {
        let hex = "ab".repeat(32);
        let query = SearchQuery::new()
            .with_digest_text(&format!("sha256:{hex}"))
            .expect("a digest");
        let digest = query.digest.expect("the digest is there");
        assert_eq!(digest.algorithm, DigestAlgorithm::Sha256);
        assert_eq!(digest.value, hex);
        for bad in ["sha256", "sha1:ab", "sha256:zz", "sha256:", "sha256:ab"] {
            assert!(
                SearchQuery::new().with_digest_text(bad).is_err(),
                "{bad:?} is not a digest"
            );
        }
    }

    #[test]
    fn a_digest_in_uppercase_is_the_same_digest() {
        let hex = "AB".repeat(32);
        let query = SearchQuery::new()
            .with_digest_text(&format!("SHA256:{hex}"))
            .expect("a digest");
        assert_eq!(query.digest.expect("the digest").value, "ab".repeat(32));
    }

    #[test]
    fn an_attribute_is_read_from_its_text() {
        let attribute = RepositoryAttribute::parse("format=pypi").expect("an attribute");
        assert_eq!(attribute.name(), "format");
        assert_eq!(attribute.value(), "pypi");
        assert_eq!(
            RepositoryAttribute::parse("channel=trusted")
                .expect("an attribute")
                .value(),
            "trusted"
        );
        assert_eq!(
            RepositoryAttribute::parse("mode=proxy")
                .expect("an attribute")
                .value(),
            "proxy"
        );
        for bad in ["format", "format=zip", "colour=red", "format="] {
            assert!(
                RepositoryAttribute::parse(bad).is_err(),
                "{bad:?} is not an attribute"
            );
        }
    }

    #[test]
    fn an_attribute_says_its_name_and_its_value() {
        let attribute = RepositoryAttribute::Format(RepositoryFormat::PyPi);
        assert_eq!(attribute.name(), "format");
        assert_eq!(attribute.value(), "pypi");
        let attribute = RepositoryAttribute::Channel(ServeChannel::TrustedOnly);
        assert_eq!(attribute.name(), "channel");
        assert_eq!(attribute.value(), "trusted");
    }
}
