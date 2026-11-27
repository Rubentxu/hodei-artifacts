# Puerta de nombres parecidos (typosquatting) en la publicación npm

HSP-322 tramo 1, desde HSP-166. Un repositorio **npm hospedado** puede declarar
una política de nombres en `config_settings.typosquat`. Al publicar un nombre
**nuevo**, el servicio lo compara con los nombres que ya existen y, antes de
escribir un solo byte, lo rechaza o lo señala.

```json
"typosquat": { "mode": "block", "protected": ["react", "lodash"], "scan_registry": true }
```

| Campo | Significado |
| --- | --- |
| `mode` | **Obligatorio.** `block`: rechaza (403). `audit`: publica y deja el evento `typosquat.flagged` (paso de despliegue). No hay modo implícito. |
| `protected` | Lista opcional que fija el administrador (máx. 1000 nombres). Es la respuesta por defecto a «de dónde salen los nombres populares»: no hay lista global incorporada. |
| `scan_registry` | Por defecto `true`: compara también con los paquetes ya publicados en **ese mismo repositorio**. Si es `false`, `protected` no puede estar vacía. |

## Decisión de seguridad

- **Sin declaración, comportamiento intacto**: no hay detector ni eventos.
- Solo se juzga un **nombre nuevo**. Una versión nueva de un paquete que ya
  existe es el paquete, no una imitación.
- **Nunca falla abierto**: una declaración ilegible, o un listado del registro
  que falla o desborda el límite del catálogo, devuelve **503** y no publica
  nada (evento `typosquat.unavailable`).
- El rechazo es **403** con un motivo genérico fijo; ni el nombre coincidente
  ni la regla salen del servicio (van al log `hodei_artifacts::typosquat`).
  Eventos de auditoría: `typosquat.refused` (denied), `typosquat.flagged`
  (allowed), `typosquat.unavailable` (failed), con recurso `repositorio/nombre`.
- Es una heurística sobre nombres, no un veredicto de malware.

## Juicio (puro, `hodei_risk::advisories::find_lookalike`)

Dos nombres se parecen si comparten *esqueleto* (minúsculas, separadores
`-`/`_`/`.` o ausentes, caracteres invisibles, formas de ancho completo,
homoglifos cirílicos y griegos, `0`/`o`, `1`/`l`, `rn`/`m`, `vv`/`w`), o si
están a distancia de edición 1 (sustitución, inserción, borrado o transposición
adyacente) y ambos nombres tienen al menos 4 caracteres.

## Despliegue

- Declararla exige `CreateRepository` (como la política de espacios de nombres):
  solo restringe, no concede ni sale a la red. **No añade acción Cedar ni
  cambia el contrato de despliegue.** Solo se acepta en repos npm hospedados;
  en cualquier otro sitio, o ilegible, es 400.
- No hay endpoint para modificar la configuración de un repositorio existente.

## Límites conocidos

- El alcance es el registro del propio repositorio: no ve otros repositorios
  de la cuenta (un principal podría no poder leerlos).
- El listado del registro está acotado por `MAX_ASSETS_UNDER` (20 000 assets,
  tarballs incluidos); por encima falla cerrado (503). Mitigación:
  `scan_registry: false` con `protected`.
- Cubre `PUT /npm/{repo}/{nombre}` (publicación). Otros formatos y la ruta de
  importación de migración no pasan por esta puerta.
- Cobertura de homoglifos: lista corta y deliberada, no UTS #39 completo.
- Un nombre idéntico a uno de `protected` no se trata como imitación: reservar
  nombres es la política de espacios de nombres (HSP-143).
