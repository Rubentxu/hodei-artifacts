# Convención de Commits

Este documento describe la convención de commits utilizada en este proyecto. El objetivo es estandarizar los mensajes de commit para que sean fáciles de entender y permitan generar automáticamente changelogs.

## Formato del mensaje de commit

El mensaje de commit debe seguir el siguiente formato:

```
type(scope): resumen
```

### Tipo

El tipo describe la naturaleza del commit. Debe ser uno de los siguientes:

*   `feat`: Introduce una nueva característica.
*   `fix`: Corrige un error.
*   `refactor`: Realiza un cambio en el código que no afecta al comportamiento externo.
*   `perf`: Mejora el rendimiento.
*   `test`: Añade o mejora tests.
*   `docs`: Cambios en la documentación.
*   `build`: Cambios en el sistema de construcción o dependencias.
*   `chore`: Tareas de mantenimiento menores.
*   `ci`: Cambios en la configuración de integración continua.
*   `revert`: Revierte un commit anterior.

### Scope

El scope indica la parte del código afectada por el commit. Puede ser:

*   El nombre de un crate (ej: `artifact`, `search`, `repository`).
*   Una capa de la arquitectura (ej: `domain`, `application`, `infrastructure`, `features`).
*   Una combinación de ambos (ej: `search-features`, `api-infrastructure`).

### Resumen

El resumen es una descripción breve y concisa del cambio realizado. Debe:

*   Estar en imperativo presente (ej: "Añadir filtro básico de búsqueda", no "Añadido filtro básico de búsqueda").
*   Empezar con mayúscula.
*   No exceder los 72 caracteres.

### Indicador Breaking Change

Si el commit introduce un cambio incompatible con versiones anteriores, se debe añadir un `!` después del scope:

```
feat(api!): Eliminar endpoint obsoleto
```

## Cuerpo del mensaje (opcional)

El cuerpo del mensaje puede contener información adicional sobre el commit. Debe estar separado del resumen por una línea en blanco.

Puede contener las siguientes secciones, en este orden:

*   `Context`: Describe el problema o la motivación del cambio.
*   `Changes`: Lista concisa de las modificaciones clave.
*   `Details`: Notas técnicas sobre la implementación.
*   `Migration`: Pasos necesarios para migrar a la nueva versión (solo si hay breaking change).
*   `Breaking-Change`: Descripción detallada del cambio incompatible.
*   `Perf`: Métricas de rendimiento (si aplica).

## Ejemplo

```
feat(search): Añadir índice invertido básico para búsqueda por nombre

Context:
La búsqueda necesitaba soporte por nombre exacto y prefijo.

Changes:
- Crear MongoSearchIndex con índice compuesto (repo_id + name_normalized)
- Añadir endpoint GET /v1/search
- Indexar nombre normalizado en inserción de repositorio
- Tests integración con datos semilla

Migration:
Recrear índices en colección repositories.

Breaking-Change:
Se renombra campo 'repoName' a 'name' en respuesta JSON.
```

## Exclusión de autoría

Los mensajes de commit no deben incluir líneas `Signed-off-by:` ni `Co-authored-by:`.

## Plantilla de commit

Se recomienda usar la siguiente plantilla para crear los mensajes de commit:

```
type(scope): resumen

Context:
-

Changes:
-

Details:
-

Migration:
-

Breaking-Change:
-
```

Para configurar Git para que use esta plantilla, ejecute el siguiente comando:

```
git config commit.template .gitmessage
```

## Hook de validación

Se ha configurado un hook `commit-msg` para validar el formato de los mensajes de commit. Si el mensaje no cumple con la convención, el commit será rechazado.

## Próximos pasos

*   Actualizar la plantilla de pull request para exigir el formato de commit.
*   Revisar la adopción de la convención tras las primeras 3 features.
