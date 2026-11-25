# Navegación del frontend

## Rutas canónicas

| Superficie                | Ruta canónica                     | Alias y destino                                   |
| ------------------------- | --------------------------------- | ------------------------------------------------- |
| Inicio / resumen          | `/`                               | `/dashboard` y `/overview` → `/`                  |
| Repositorios del catálogo | `/console/repositories`           | `/catalog/repositories` → `/console/repositories` |
| Explorador de artefactos  | `/console/browse`                 | `/browse` → `/console/browse`                     |
| Detalle de paquete        | `/console/packages/:repository/*` | Sin alias                                         |
| Administración            | `/admin`                          | Sin alias                                         |
| Registro de repositorios  | `/repositories`                   | `/admin/registry` → `/repositories`               |
| Usuarios                  | `/admin/users`                    | `/users` → `/admin/users`                         |
| Tokens                    | `/settings/tokens`                | `/tokens` → `/settings/tokens`                    |
| Políticas de acceso       | `/settings/policies`              | `/policies` → `/settings/policies`                |
| Perfil                    | `/profile`                        | Sin alias                                         |
| Demo de OpenAPI           | `/openapi-demo`                   | Sin alias                                         |
| Inicio de sesión          | `/login`                          | Sin alias                                         |

El registro (`/repositories`) y los repositorios del catálogo (`/console/repositories`)
son superficies distintas: el primero administra el registro de API y el segundo
la consola del catálogo. Por eso `/admin/registry` redirige al registro y nunca
al catálogo. Las rutas de políticas también tienen un único destino: `/policies`
es un alias histórico y no una segunda implementación.

## Navegación móvil y teclado

El menú móvil se presenta como un `<dialog>` modal nativo. Al abrirse, el foco
pasa al primer enlace; la navegación por tabulación queda contenida en el diálogo,
y Escape lo cierra y devuelve el foco al botón que lo abrió. El viewport estrecho,
el orden de tabulación y la restauración del foco se comprueban en la prueba E2E
de accesibilidad.
