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

## Rotación de claves

Se rota cambiando `apt.trusted_key_asc` en la configuración del repositorio (decisión del operador, como
hasta ahora). La marca monótona sigue vigente: si el upstream re-emite con la nueva clave un `Release`
con `Date` mayor, se acepta sin más; si la `Date` no avanza, hay que hacer `reset` explícito. No hay
cadena de raíces TUF ni delegación: ver limitaciones.

## Contrato de despliegue

- El perfil de grafo crea el ledger al arrancar (predicados aditivos `hodei.mirrormeta.*`, sin tocar el
  esquema versionado). Un repo proxy apt con `trusted_key_asc` pasa a **exigir** ese ledger y la `Date`
  del `Release`: upstreams cuyo `Release` no lleva `Date` dejan de servirse.
- Las políticas Cedar y los schemas de despliegue deben declarar `ManageMirrorMetadata`
  (`config/*/schema.json` ya lo hacen); el arranque ya verifica las acciones declaradas.
- Sin `trusted_key_asc` el proxy sigue sin autenticar su índice y **no** tiene esta protección.

## Límites (no demostrados / fuera de alcance)

- Solo apt proxy con clave anclada. Yum/RPM, cargo sparse, npm, OCI y demás no tienen esta capa.
- La monotonía es la `Date` del `Release` (resolución de segundo), no una versión TUF; no hay roles
  `timestamp`/`snapshot`/`targets` ni raíz delegada con rotación por umbral.
- El ledger es append-only: una fila por `Release` nuevo aceptado (sin poda).
- Un upstream offline autorizado (`cache.offline`) y su snapshot firmado con fecha no están cubiertos.
