# SPEC F-001 — Functional Core / Imperative Shell en Rust
**Objetivo:** arquitectura hexagonal+vertical slice sin frameworks ni herencia simulada, inspirada en Haskell.
**Contexto anterior:** `docs/prd/4-bounded-contexts-and-crates-structure.md`, `docs/architecture.md`.

## Reglas vinculantes
1. Algebraic Data Types (`enum` discriminado, newtypes, structs opacas con smart constructor, `Result<T, DomainError>`). `Option` solo ausencia legítima, no para tapar validación. Restricciones de estado se representan por tipos siempre que sea rentable.
2. `fn decide(state: &State, command: &Command, facts: &VerifiedFacts) -> Result<Vec<Event>, DomainError>` y `fn evolve(State, Event) -> State` puros para reglas con transiciones. `VerifiedFacts` no contiene referencias mutables a recursos externos; su origen/admisión se comprueba en application.
3. Reloj, UUID, aleatoriedad, I/O, HTTP, TypeDB, FS, S3, Cedar, métricas, secretos, parser de bytes de red están fuera del core o del flujo de efectos. `Future`/`async` solo en aplicación y adaptadores salvo funciones asincrónicas puramente computacionales justificadas.
4. No llamar `std::time::SystemTime::now()`, `OffsetDateTime::now_utc`, `Uuid::new_v4`, `tokio`, logging ni DB dentro de funciones puras. Pasar timestamp y random IDs como entrada.
5. Domain events son descripciones tipadas de transiciones; no se exige un Event Store universal. Los eventos auditable/durable jobs se confirman atomizados con el estado TypeDB cuando sea necesario.
6. Seams: `api/http/protocol` -> `features/<capability>/{domain,application,ports,adapters}`; cada slice posee su contrato y sus pruebas, pero NO exige archivos vacíos `dto.rs`, `di.rs`, `mocks.rs` por regla rígida. Compartir kernel solo tipos realmente estables, otros se copian localmente o se publican mediante contratos.
7. Encapsular puertos en application; `trait` de capacidades concretas (`AssetReader`, `BlobWriter`, `AuthorizationService`) con métodos cohesionados, sin repositorio universal ni God Services. Para un solo backend inevitable no introducir DI compleja gratuita.
8. Refactor seguro por API observable; dominios independientes de frameworks. `unsafe` no autorizado salvo evidencia y revisión específica.
9. Donde `Vec<u8>` sea inevitable: límites explícitos y solo metadatos pequeños; bodies grandes por `Stream<Item=Result<Bytes, Error>>` y control de backpressure.
10. Evitar duplicación entre `artifact`, `repository`, `supply-chain`; propietario semántico por tipo definido en `ontology.md`.

## Ejemplo de contratos conceptuales (adaptar a Rust compilable real)
```rust
pub enum RepositoryKind { Hosted(HostedPolicy), Proxy(ProxyPolicy), Group(GroupPolicy) }
pub struct Repository { id: RepositoryId, tenant: TenantId, kind: RepositoryKind }
pub enum PublishError { Forbidden, ReadOnly, Conflict, InvalidDigest, InvalidMetadata }
pub struct VerifiedBlob { digest: Digest, bytes: u64, storage_receipt: StorageReceipt }
pub struct VerifiedFacts { principal: PrincipalId, now: Timestamp, authorization: AuthorizationGrant }
pub fn decide_publish(
  repo: &Repository, cmd: &Publish, blob: &VerifiedBlob, facts: &VerifiedFacts
) -> Result<Vec<AssetEvent>, PublishError> { /* reglas puras */ todo!() }
```
El ejemplo es **especificación conceptual**, no código ejecutado; no copiar placeholders a producción.

## Tests más baratos
Propiedades: `parse/render` HRN, estados válidos, idempotencia de `evolve`, equivalencia de serialización y errores tipados. Data-driven: redeploy, nombres Unicode/rutas, TTL, transiciones. Integración solo en los puertos de persistencia/seguridad y un caso de composición por vertical.

## Criterio de salida
Dominio compilable unitario sin depender de `axum`, `cedar-policy`, `typedb-driver` o SDK S3; un nuevo formato se añade sin modificar la ontología de blobs; la API previa queda versionada o migrada.
