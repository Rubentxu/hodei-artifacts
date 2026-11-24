//! La readiness del despliegue: que dependencias exige el perfil activo y
//! si estan disponibles.
//!
//! La readiness **no** es la liveness. La liveness dice que el proceso
//! vive; la readiness dice que el proceso sabe servir: que las
//! dependencias que el perfil activo exige --el catalogo y el almacen de
//! contenido-- estan a mano. Un despliegue sin catalogo no puede decir
//! que repositorio existe, y un despliegue sin almacen de contenido no
//! tiene bytes que servir: en los dos casos la readiness es un `503`, y el
//! proceso sigue vivo.
//!
//! El puerto vive aqui y no en el handler por dos razones:
//!
//! - el handler no tiene estado: el presupuesto de red y el cache corto
//!   estan en el envoltorio, y se pueden probar sin un servidor;
//! - el cuerpo de la respuesta es **grueso** a proposito: dice que
//!   dependencia falla y como, pero nunca donde esta ni con que se
//!   autentica.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;

/// El presupuesto de red de **una** dependencia.
///
/// Es corto a proposito: la readiness la consulta un orquestador cada
/// pocos segundos, y una comprobacion que se cuelga es una comprobacion
/// que se acumula. Un segundo basta para notar que un almacen local no
/// esta y que un bucket remoto no contesta.
pub const DEPENDENCY_BUDGET: Duration = Duration::from_secs(1);

/// El presupuesto de red de **todo** el probe.
///
/// Con dos dependencias exigidas y un presupuesto de un segundo cada una,
/// el probe entero no pasa de dos segundos. Un despliegue que exija mas
/// dependencias que presupuesto solo comprueba las que caben, y las
/// demas se declaran `Timeout`: es mejor decir que no se pudo comprobar
/// que mentir diciendo que estan bien.
pub const PROBE_BUDGET: Duration = Duration::from_secs(2);

/// El tiempo que un veredicto se cachea.
///
/// Es corto a proposito: un segundo es suficiente para que dos sondeos
/// seguidos --un orquestador que pregunta cada poco-- no gasten el
/// presupuesto dos veces, y es lo bastante corto para que la
/// recuperacion de una dependencia se vea al instante.
pub const VERDICT_TTL: Duration = Duration::from_secs(1);

/// Una dependencia que el perfil activo exige para servir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dependency {
    /// El catalogo: los repositorios, los assets y las versiones.
    Catalog,
    /// El almacen de contenido: los bytes.
    ContentStore,
    /// El almacen de confianza, cuando su lectura ocurre en caliente.
    ///
    /// En este despliegue la politica de confianza se carga en memoria al
    /// arrancar: no se puede perder en caliente, y una comprobacion que
    /// siempre sale bien es una mentira. Por eso no lo exige nadie.
    TrustPolicy,
}

impl Dependency {
    /// El nombre de la dependencia, como va en el cuerpo de la respuesta.
    ///
    /// Es el **nombre** y no la direccion: el cuerpo dice que dependencia
    /// falla, no donde esta.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::ContentStore => "content_store",
            Self::TrustPolicy => "trust_policy",
        }
    }
}

/// Por que una dependencia exigida no esta disponible.
///
/// Grueso a proposito: el cuerpo de la respuesta no puede llevar ni una
/// URL, ni un bucket, ni una ruta, ni una credencial. El error crudo del
/// driver puede traer las tres cosas --un error de S3 trae el endpoint y
/// a veces la credencial--, asi que no viaja.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyFailure {
    /// No se puede alcanzar.
    Unreachable,
    /// No contesta dentro del presupuesto.
    Timeout,
    /// Esta declarada de una forma que no se puede usar.
    Misconfigured,
}

impl DependencyFailure {
    /// El estado de la dependencia, como va en el cuerpo de la respuesta.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Timeout => "timeout",
            Self::Misconfigured => "misconfigured",
        }
    }
}

/// El veredicto del despliegue.
///
/// Es grueso como el motivo del fallo: dice **que** dependencia no esta,
/// y no dice nada mas.
#[derive(Debug, Clone)]
pub struct ReadinessReport {
    /// Si el despliegue puede servir.
    pub ready: bool,
    /// Las dependencias exigidas que fallan, con su motivo.
    pub failing: Vec<(Dependency, DependencyFailure)>,
}

/// El comprobador de las dependencias exigidas por el perfil activo.
#[async_trait]
pub trait ReadinessProbe: Send + Sync {
    /// Las dependencias que el perfil activo exige.
    ///
    /// Las que no estan aqui no se comprueban y no pueden hacer fallar el
    /// readiness: una dependencia opcional que el perfil no exige no es
    /// una dependencia.
    fn required(&self) -> Vec<Dependency>;

    /// Comprueba **una** dependencia.
    ///
    /// Un error de la dependencia **no** es un panic ni un `500`: es un
    /// estado de readiness.
    async fn check(&self, dependency: Dependency) -> Result<(), DependencyFailure>;

    /// El veredicto del despliegue.
    ///
    /// El metodo por defecto es el que aplica el presupuesto: cada
    /// dependencia tiene su propio plazo y el probe entero tiene el suyo.
    /// Un probe que devuelve un veredicto sin presupuesto --uno de
    /// mentira-- lo dice aqui.
    async fn report(&self) -> ReadinessReport {
        let mut failing = Vec::new();
        let deadline = Instant::now() + PROBE_BUDGET;
        for dependency in self.required() {
            // El presupuesto del probe manda sobre el de la dependencia:
            // lo que no cabe no se comprueba, y se declara `Timeout`.
            if Instant::now() >= deadline {
                failing.push((dependency, DependencyFailure::Timeout));
                continue;
            }
            let outcome = tokio::time::timeout(DEPENDENCY_BUDGET, self.check(dependency)).await;
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(failure)) => failing.push((dependency, failure)),
                Err(_elapsed) => failing.push((dependency, DependencyFailure::Timeout)),
            }
        }
        ReadinessReport {
            ready: failing.is_empty(),
            failing,
        }
    }
}

/// Un veredicto cacheado, con el instante en que se tomo.
struct Cached {
    at: Instant,
    report: ReadinessReport,
}

/// El probe con el cache corto del veredicto.
///
/// El cache va **aqui** y no en el handler: el handler no tiene estado, y
/// el probe se puede probar solo. Dos readiness seguidos dentro del TTL
/// no gastan presupuesto: el segundo devuelve el veredicto cacheado.
pub struct CachedProbe {
    inner: Arc<dyn ReadinessProbe>,
    ttl: Duration,
    cache: Mutex<Option<Cached>>,
}

impl CachedProbe {
    /// El probe real, envuelto en el cache corto.
    pub fn new(inner: Arc<dyn ReadinessProbe>) -> Self {
        Self::with_ttl(inner, VERDICT_TTL)
    }

    /// El probe real, envuelto en un cache del plazo que se le dice.
    ///
    /// El plazo por defecto es el de [`VERDICT_TTL`]; el que se le pasa es
    /// para las pruebas, que no pueden dormir un segundo por caso.
    pub fn with_ttl(inner: Arc<dyn ReadinessProbe>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            cache: Mutex::new(None),
        }
    }

    /// El probe real que este envuelve.
    pub fn the_inner(&self) -> &Arc<dyn ReadinessProbe> {
        &self.inner
    }
}

#[async_trait]
impl ReadinessProbe for CachedProbe {
    fn required(&self) -> Vec<Dependency> {
        self.inner.required()
    }

    async fn check(&self, dependency: Dependency) -> Result<(), DependencyFailure> {
        self.inner.check(dependency).await
    }

    /// El veredicto, cacheado.
    async fn report(&self) -> ReadinessReport {
        {
            let cache = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(cached) = cache.as_ref()
                && cached.at.elapsed() < self.ttl
            {
                return cached.report.clone();
            }
        }
        let report = self.inner.report().await;
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *cache = Some(Cached {
            at: Instant::now(),
            report: report.clone(),
        });
        report
    }
}

/// Como se comprueba el almacen de contenido.
///
/// El perfil lo decide en el arranque: un despliegue con el contenido en
/// un directorio y un despliegue con el contenido en un bucket no se
/// comprueban igual, y el que lo dice es la configuracion.
pub enum ContentCheck {
    /// Un directorio del sistema de archivos.
    Filesystem {
        /// El directorio donde vive el contenido.
        root: PathBuf,
    },
    /// Un bucket de S3.
    S3 {
        /// El cliente, ya configurado con la cadena de credenciales.
        client: aws_sdk_s3::Client,
        /// El bucket donde vive el contenido.
        bucket: String,
    },
}

impl ContentCheck {
    /// Comprueba el almacen de contenido.
    ///
    /// El directorio se comprueba con una llamada al sistema de archivos:
    /// existe y es un directorio. No se lee nada de dentro: la readiness
    /// no necesita el contenido, necesita el sitio donde vive.
    pub async fn check(&self) -> Result<(), DependencyFailure> {
        match self {
            Self::Filesystem { root } => {
                if root.is_dir() {
                    return Ok(());
                }
                if root.exists() {
                    // Hay algo ahi, pero no es el directorio del almacen.
                    return Err(DependencyFailure::Misconfigured);
                }
                Err(DependencyFailure::Unreachable)
            }
            Self::S3 { client, bucket } => {
                // `head_bucket` es la llamada mas barata que dice a la vez
                // que el bucket existe y que las credenciales sirven. No se
                // lista el bucket: un listado bajaria el catalogo de
                // objetos, y la readiness no necesita los objetos.
                match client.head_bucket().bucket(bucket).send().await {
                    Ok(_) => Ok(()),
                    Err(error) => {
                        // El codigo dice si el bucket **no** es nuestro o
                        // si no se pudo alcanzar. Lo que no se dice nunca es
                        // por que: el error del driver puede traer el
                        // endpoint y la credencial.
                        let status = error
                            .raw_response()
                            .map(|response| response.status().as_u16());
                        Err(match status {
                            Some(403) | Some(404) => DependencyFailure::Misconfigured,
                            _ => DependencyFailure::Unreachable,
                        })
                    }
                }
            }
        }
    }
}

/// El probe del perfil activo: las dependencias que exige el despliegue y
/// como se comprueba cada una.
///
/// Las que no estan aqui no se exigen: una dependencia que el perfil no
/// exige no puede hacer fallar el readiness.
pub struct ProfileProbe {
    catalog: Option<Surreal<Db>>,
    content: Option<ContentCheck>,
}

impl ProfileProbe {
    /// Un perfil que no exige nada.
    pub fn new() -> Self {
        Self {
            catalog: None,
            content: None,
        }
    }

    /// El perfil exige el catalogo, que vive en este motor.
    pub fn with_catalog(mut self, db: Surreal<Db>) -> Self {
        self.catalog = Some(db);
        self
    }

    /// El perfil exige el almacen de contenido.
    pub fn with_content(mut self, content: ContentCheck) -> Self {
        self.content = Some(content);
        self
    }
}

impl Default for ProfileProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ReadinessProbe for ProfileProbe {
    fn required(&self) -> Vec<Dependency> {
        let mut required = Vec::new();
        if self.catalog.is_some() {
            required.push(Dependency::Catalog);
        }
        if self.content.is_some() {
            required.push(Dependency::ContentStore);
        }
        required
    }

    async fn check(&self, dependency: Dependency) -> Result<(), DependencyFailure> {
        match dependency {
            Dependency::Catalog => {
                let Some(db) = self.catalog.as_ref() else {
                    return Err(DependencyFailure::Misconfigured);
                };
                // El catalogo de este despliegue es el motor embebido: esta
                // dentro del proceso. Su comprobacion es **trivial** --una
                // sentencia que no toca ninguna tabla y no baja el
                // catalogo-- y su fallo no es alcanzable en caliente. Un
                // catalogo **remoto** si se puede perder en caliente, y su
                // comprobacion seria la lectura mas barata del puerto del
                // catalogo, con una pagina de uno.
                let mut response = db
                    .query("RETURN true")
                    .await
                    .map_err(|_error| DependencyFailure::Unreachable)?;
                if response.take_errors().is_empty() {
                    Ok(())
                } else {
                    Err(DependencyFailure::Unreachable)
                }
            }
            Dependency::ContentStore => {
                let Some(content) = self.content.as_ref() else {
                    return Err(DependencyFailure::Misconfigured);
                };
                content.check().await
            }
            Dependency::TrustPolicy => Err(DependencyFailure::Misconfigured),
        }
    }
}

/// El probe de un despliegue que no exige ninguna dependencia.
///
/// Es el probe por defecto del estado: un despliegue que no ha declarado
/// que exige nada esta listo, y decirlo es mejor que inventar una
/// dependencia que comprobar.
pub struct NothingRequired;

#[async_trait]
impl ReadinessProbe for NothingRequired {
    fn required(&self) -> Vec<Dependency> {
        Vec::new()
    }

    async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
        Err(DependencyFailure::Misconfigured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Un probe de mentira que cuenta las comprobaciones y falla donde se
    /// le dice.
    struct Counting {
        required: Vec<Dependency>,
        checks: AtomicUsize,
        answer: Result<(), DependencyFailure>,
    }

    impl Counting {
        fn new(required: Vec<Dependency>, answer: Result<(), DependencyFailure>) -> Self {
            Self {
                required,
                checks: AtomicUsize::new(0),
                answer,
            }
        }

        fn checks(&self) -> usize {
            self.checks.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl ReadinessProbe for Counting {
        fn required(&self) -> Vec<Dependency> {
            self.required.clone()
        }

        async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
            self.checks.fetch_add(1, Ordering::Relaxed);
            self.answer
        }
    }

    /// Un probe que no contesta nunca.
    struct Silent;

    #[async_trait]
    impl ReadinessProbe for Silent {
        fn required(&self) -> Vec<Dependency> {
            vec![Dependency::ContentStore]
        }

        async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn una_dependencia_que_no_contesta_produce_timeout_y_no_un_500() {
        let probe = CachedProbe::with_ttl(Arc::new(Silent), Duration::from_millis(10));
        let report = probe.report().await;
        assert!(!report.ready);
        assert_eq!(
            report.failing,
            vec![(Dependency::ContentStore, DependencyFailure::Timeout)]
        );
    }

    #[tokio::test]
    async fn una_dependencia_opcional_que_no_se_exige_no_puede_fallar_el_readiness() {
        // El probe exige el catalogo y la comprobacion falla: el veredicto
        // es que no esta listo. La dependencia que no se exige --el
        // almacen de contenido-- no aparece ni puede fallar.
        let probe = CachedProbe::with_ttl(
            Arc::new(Counting::new(
                vec![Dependency::Catalog],
                Err(DependencyFailure::Unreachable),
            )),
            Duration::from_millis(10),
        );
        let report = probe.report().await;
        assert!(!report.ready);
        assert_eq!(
            report.failing,
            vec![(Dependency::Catalog, DependencyFailure::Unreachable)]
        );
        assert_eq!(probe.required(), vec![Dependency::Catalog]);
    }

    #[tokio::test]
    async fn el_cache_corto_no_repite_la_comprobacion() {
        let counting = Arc::new(Counting::new(vec![Dependency::ContentStore], Ok(())));
        let probe = CachedProbe::with_ttl(counting.clone(), Duration::from_secs(30));
        let first = probe.report().await;
        let second = probe.report().await;
        assert!(first.ready);
        assert!(second.ready);
        assert_eq!(counting.checks(), 1, "el cache corto no vuelve a comprobar");
    }

    #[tokio::test]
    async fn el_probe_no_consulta_mas_de_una_vez_cada_dependencia() {
        let counting = Arc::new(Counting::new(
            vec![Dependency::Catalog, Dependency::ContentStore],
            Ok(()),
        ));
        let probe = CachedProbe::with_ttl(counting.clone(), Duration::from_millis(10));
        let report = probe.report().await;
        assert!(report.ready);
        assert_eq!(counting.checks(), 2, "una comprobacion por dependencia");
    }
}
