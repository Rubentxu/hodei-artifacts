# Plantilla de notas de versión

Plantilla para las notas de cada versión de Hodei Artifacts, escritas para quien
**usa y opera** el servicio, no para quien lo desarrolla. Copia el bloque de la
sección 2, sustituye lo que está entre `<...>` y **borra** las instrucciones
en cursiva y las secciones que no apliquen dejando, en su lugar, "Sin cambios"
(una sección que falta no dice nada; una que dice "Sin cambios" afirma algo).

Reglas de la plantilla:

- **Solo afirmaciones comprobadas.** Cada línea de "Verificación" debe poder
  ejecutarla el lector. No copies un comando que no hayas ejecutado contra
  ese release.
- **Lo que no funciona se dice.** Si una capacidad queda como "no disponible
  todavía", va a la sección de límites, no se omite.
- **El historial vive en el changelog.** Estas notas resumen y enlazan; no
  sustituyen a [`CHANGELOG.md`](../../CHANGELOG.md) ni crean un seguimiento
  paralelo de tareas.

## 1. Dónde encaja cada sección

| Sección de la nota | De dónde sale la información | Referencia |
| --- | --- | --- |
| Cambios | `CHANGELOG.md` (sección de la versión) | [`CHANGELOG.md`](../../CHANGELOG.md) |
| Cambios incompatibles | Diferencias en claves, variables y rutas | [`configuration.md`](configuration.md) |
| Actualización | Procedimiento manual y su estado | [`operations.md`](operations.md) |
| Verificación de artefactos | El script de verificación y el manifiesto del release | [`scripts/verify-release.sh`](../../scripts/verify-release.sh) |
| Compatibilidad por ecosistema | Matriz de capacidades | [`ecosystems.md`](ecosystems.md) |

## 2. Plantilla

````markdown
# Hodei Artifacts <versión> — <AAAA-MM-DD>

_Resumen de una o dos frases: qué cambia para quien lo usa._

## Cambios

### Novedades
- <qué puede hacer ahora el usuario, en una línea, con el ecosistema o la superficie>

### Correcciones
- <qué fallaba, qué ocurre ahora>

### Seguridad
- <cambio que afecta a la exposición del servicio, a secretos o a firmas>

## Cambios incompatibles

_Todo lo que obliga a actuar antes o durante la actualización. Si no hay,
escribe "Sin cambios incompatibles"._

- <clave, variable de entorno, ruta HTTP o formato que cambió o se retiró>
  - **Antes:** <valor o comportamiento anterior>
  - **Ahora:** <valor o comportamiento nuevo>
  - **Qué hacer:** <acción concreta>

## Actualización

_Desde la versión <anterior>. Sigue la secuencia de
[`operations.md`](operations.md), sección "Actualización y rollback"._

1. Detén el servicio y haz una copia verificada (`hodei-recovery backup` y
   `hodei-recovery verify`).
2. Verifica el release (sección siguiente).
3. Aplica los cambios de configuración de "Cambios incompatibles".
4. Despliega el paquete nuevo y arranca el servicio.
5. Comprueba `GET /health`.

**Rollback:** <"No disponible todavía; la copia del paso 1 es la red de
seguridad" mientras así sea en operations.md, o el procedimiento probado>.

## Verificación de artefactos

| Artefacto | Fichero | SHA-256 |
| --- | --- | --- |
| Paquete | `hodei-artifacts-api-<versión>-<target>.tar.gz` | `<hash>` |
| Imagen OCI | `hodei-artifacts-api-<versión>-<arq>-oci.tar` | `<hash>` (digest `<sha256:...>`) |
| SBOM (SPDX, CycloneDX) | `*-oci.spdx.json`, `*-oci.cdx.json`, `*-cargo.spdx.json` | `<hash>` |
| Licencias | `*-licenses.json` | `<hash>` |
| Procedencia | `*.provenance.json` | `<hash>` |
| Manifiesto | `*.manifest.json` | `<hash>` |

- **Huella de la clave de firma:** `<huella>` (modo: `<ephemeral-local | operator-keyring>`).
  Compárala con la publicada por el mantenedor por un canal independiente.
- **Commit:** `<sha>`.

```bash
sha256sum -c SHA256SUMS
scripts/verify-release.sh <directorio-dist>
```

El segundo comando termina con una línea `OK: verified ...`; cualquier otra
salida significa que **no** debes desplegar el release.

## Compatibilidad por ecosistema

_Copia la matriz de capacidades de [`ecosystems.md`](ecosystems.md) solo si
cambió; si no, enlázala._

## Límites conocidos

- <capacidad que sigue "no disponible todavía" y que el lector podría esperar>

## Créditos y enlaces

- Cambios completos: `CHANGELOG.md`, sección <versión>.
````

## 3. Lista de comprobación antes de publicar las notas

- [ ] Cada enlace de la nota resuelve (en este repositorio, los de
      `docs/community/` los valida la prueba `hsp243_user_guides`).
- [ ] Los hashes y la huella son los del manifiesto del release, no de otro.
- [ ] Las órdenes de "Verificación" se ejecutaron contra ese release.
- [ ] "Cambios incompatibles" menciona cada clave o variable de
      [`configuration.md`](configuration.md) que cambió.
- [ ] No aparece ningún secreto, URL interna ni ruta de una máquina concreta.
