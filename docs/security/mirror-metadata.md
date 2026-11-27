# Protección rollback/freeze del índice firmado de un proxy apt (HSP-167, tramo mínimo)

Una firma OpenPGP prueba *quién* escribió el `Release`, no *cuándo*. Un `Release` antiguo pero
válidamente firmado (replay) ancla a los clientes en paquetes vulnerables, y un upstream que deja de
publicar mantiene viva una firma vieja. Este tramo añade, **solo a los proxies apt con clave anclada**,
protección contra rollback y freeze, sin segundo sistema de firma.

## Qué hace

Cuando el repositorio proxy declara `config_settings.apt.trusted_key_asc` (la clave que el **operador**
ancló; nunca una que anuncie el upstream), cada `InRelease`/`Release`, `Packages` y `pool/` que consume
ese índice pasa por:

1. verificación de la firma contra la clave anclada (ya existente);
2. rechazo de freeze: `Valid-Until` vencido, `Date` más antigua que `apt.max_metadata_age_secs`
   (opcional, la define el operador), `Date` futura (más de 5 minutos) o ausente;
3. rechazo de rollback: la `Date` firmada más alta aceptada se persiste en Dgraph por
   (cuenta, repositorio, distribución). Una `Date` menor es `E_METADATA_ROLLBACK`; la misma `Date` con
   contenido distinto es `E_METADATA_FORK`; el mismo `Release` otra vez es idempotente.

Una sola resolución: lo que se sirve es exactamente el activo (direccionado por digest) que se verificó y
juzgó. `InRelease` se sirve del `InRelease` verificado; `Release` y `Release.gpg` pasan por la misma
verificación (par desprendido) y la misma guarda, así que un upstream con `InRelease` nuevo y `Release`
viejo nunca sirve el viejo. La `Date` se lee del texto que la firma cubre (`InRelease`) o de los bytes
verificados (`Release`), no del texto que rodea la armadura. Si una resolución se rechaza (502), se
reintenta una vez con refetch forzado de los metadatos, de modo que un replay servido una sola vez o una
carrera de refresco de caché no envenenan la respuesta siguiente; un 503 del ledger no se reintenta.
El digest que compara la marca es el del cuerpo verificado normalizado, común a `InRelease` y `Release`,
así que las dos formas del mismo `Release` no se ven como un fork entre sí. Un `Release` rechazado puede
quedar almacenado en el CAS, pero nunca se sirve.

En el grupo apt y en `pool/`, una distribución cuyo índice no es confiable (rollback, congelada, caída o
ausente) se omite: no sirve nada, pero no impide servir el paquete de otra distribución.

Todas las negativas son `502` con código estable (`E_METADATA_ROLLBACK|FORK|FROZEN|FUTURE|NO_DATE`) y se
auditan como `artifacts:VerifyMirrorMetadata` (`Denied`). Si el ledger no responde: `503
E_METADATA_LEDGER` (falla cerrado, no se sirve el índice). Una firma inválida nunca llega al ledger, así
que un atacante no puede envenenar la marca con una fecha lejana.

## Superficie del operador

| Operación | Ruta | Autorización |
| --- | --- | --- |
| Estado | `GET /api/v1/mirror-metadata?repository=&distribution=` | acción Cedar dedicada `Artifacts::Action::"ManageMirrorMetadata"` |
| Re-baseline | `POST /api/v1/mirror-metadata/reset` `{repository, distribution}` | la misma acción; auditado (`Allowed`/`Denied`/`Failed`) |

`reset` es la única vía de vuelta cuando el upstream retrocede legítimamente (mirror restaurado,
re-emisión tras rotar la clave). No acepta bytes: añade un paso de re-baseline y el siguiente `Release`
que pase la firma anclada pasa a ser la nueva marca. Cualquier otra cuenta recibe `404`.

## Rotación de claves (HSP-324)

Una rotación es una ventana con fecha de corte declarada por el operador en `config_settings.apt`:

```json
"apt": {
  "trusted_key_asc": "<clave anclada actual>",
  "key_rotation": { "next_key_asc": "<clave nueva>", "cutoff": "2026-12-15T00:00:00Z" }
}
```

- **Antes de `cutoff`** se aceptan las firmas de las dos claves (anclada y nueva), en `InRelease` y
  en `Release` + `Release.gpg`.
- **Desde `cutoff`** (el instante exacto ya cuenta como pasado) solo la nueva. Una firma de la clave
  retirada se rechaza con `502` y código propio `E_METADATA_KEY_RETIRED` (auditada como
  `artifacts:VerifyMirrorMetadata` `Denied`), distinto del genérico "signature is invalid" que recibe
  una tercera clave desconocida. Nunca toca la marca del ledger.
- El corte se evalúa **con el reloj del servidor**, una vez por resolución. Ni una cabecera `Date`
  del cliente, ni la `Date` del `Release`, ni el upstream lo mueven; una clave retirada no puede
  reactivarse desde el cliente: solo la edición de la configuración por el operador (con la
  autoridad que ya tiene) cambia las claves vigentes.
- Para cerrar la rotación, tras el corte el operador pone la clave nueva en `trusted_key_asc` y
  elimina `key_rotation`.
- Fallo cerrado y arranque estricto: los campos desconocidos de `apt` se rechazan, `key_rotation`
  exige `trusted_key_asc`, un `cutoff` que no sea RFC 3339 o una clave que no sea un certificado
  permitido (Ed25519) o que sea igual a la anclada se rechazan al **declarar el repositorio**
  (`POST /api/v1/catalog/repositories` responde `400`). Una declaración inválida que ya estuviera
  almacenada no sirve nada (`502`), nunca se interpreta con generosidad.

### Efecto sobre el ledger anti-rollback

El ledger no guarda con qué clave se firmó: guarda `Date` y digest del cuerpo verificado. Por tanto:

- Un `Release` aceptado con la clave vieja antes del corte **sigue siendo la marca** tras el corte
  (la marca no se invalida ni se reinicia al rotar). No hace falta `reset`.
- Pero la verificación se repite en cada petición: tras el corte ese mismo `Release` firmado con la
  clave vieja **ya no se sirve** (`E_METADATA_KEY_RETIRED`) aunque estuviera aceptado y en caché.
- El upstream que re-firma con la clave nueva el mismo contenido (misma `Date`, mismo digest) es
  idempotente: se sirve sin avanzar la marca. Con `Date` mayor, avanza. Con `Date` menor es
  `E_METADATA_ROLLBACK` como siempre; con la misma `Date` y otro contenido, `E_METADATA_FORK`.
- Si el upstream re-emite con la nueva clave pero con `Date` que no avanza y otro contenido, sigue
  haciendo falta `reset` explícito (sin cambios respecto a HSP-167).

### Cota de antigüedad por defecto

Un proxy con `trusted_key_asc` que no fija `apt.max_metadata_age_secs` recibe el valor por defecto
**604 800 s (7 días)** (`DEFAULT_MAX_METADATA_AGE_SECS`): un `Release` con `Date` más antigua es
`E_METADATA_FROZEN`. Hoy existe un único perfil productivo (el de grafo, HSP-314 F4), así que el
valor por defecto es único y vive en el código; no hay un valor distinto por perfil porque no hay
más perfiles. El operador lo sobrescribe por repositorio con un valor explícito (y puede dar uno muy
grande, acotado a ~100 años, para "sin límite práctico"). Sin `trusted_key_asc` no hay cota ni
protección.

**Compatibilidad:** los repos que ya anclaban clave sin `max_metadata_age_secs` pasan a rechazar
metadatos de más de 7 días; un upstream que publica con menos frecuencia (p. ej. un snapshot
fijo) debe fijar un valor explícito mayor.

## Contrato de despliegue

- El perfil de grafo crea el ledger al arrancar (predicados aditivos `hodei.mirrormeta.*`, sin tocar el
  esquema versionado). Un repo proxy apt con `trusted_key_asc` pasa a **exigir** ese ledger y la `Date`
  del `Release`: upstreams cuyo `Release` no lleva `Date` dejan de servirse.
- Las políticas Cedar y los schemas de despliegue deben declarar `ManageMirrorMetadata`
  (`config/*/schema.json` ya lo hacen); el arranque ya verifica las acciones declaradas.
- Sin `trusted_key_asc` el proxy sigue sin autenticar su índice y **no** tiene esta protección.

## Límites (no demostrados / fuera de alcance)

- **TOFU tras `reset`:** el siguiente `Release` con firma válida pasa a ser la marca, sea cual sea su
  `Date`; un upstream comprometido con la clave anclada puede fijar entonces una marca antigua. El reset
  es un acto del operador, auditado.
- **`Date` futura:** se tolera hasta 300 s sobre el reloj del servicio; una `Date` apenas futura fija una
  marca que rechaza sucesores legítimos hasta alcanzarla (como mucho 5 minutos).
- Dos `Release` legítimos con la misma `Date` (resolución de un segundo) y contenido distinto se rechazan
  como fork.
- La ventana de rotación acepta dos claves de forma simultánea, sin umbral ni delegación; la clave
  nueva no se prueba contra un anclaje independiente (es lo que el operador declara).
- Los miembros de un grupo apt usan la misma verificación (`verified_release`), pero la ventana solo
  está demostrada por el router sobre un proxy directo.
- `max_metadata_age_secs` se acota a unos 100 años: un valor absurdo equivale a "sin límite práctico".
- El subject del ledger escapa `%` y `/` de cuenta, repositorio y distribución.

- Solo apt proxy con clave anclada. Yum/RPM, cargo sparse, npm, OCI y demás no tienen esta capa.
- La monotonía es la `Date` del `Release` (resolución de segundo), no una versión TUF; no hay roles
  `timestamp`/`snapshot`/`targets` ni raíz delegada con rotación por umbral.
- El ledger es append-only: una fila por `Release` nuevo aceptado (sin poda).
- Un upstream offline autorizado (`cache.offline`) y su snapshot firmado con fecha no están cubiertos.
