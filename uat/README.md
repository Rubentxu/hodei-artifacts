# Los contratos de aceptación

Este directorio tiene los **contratos de aceptación** del repositorio: lo
que el producto promete, con la versión con la que se ha medido y la lista
de lo que **no** se promete.

Un contrato de aceptación no es un informe: un informe dice lo que pasó en
una ejecución, y un contrato dice lo que el registro es. La diferencia es
que el contrato se puede comprobar, y por eso cada uno de ellos tiene una
prueba que lo comprueba y una lista de lo que deja fuera.

| Documento | Superficie | Qué promete |
| --- | --- | --- |
| [`UAT-OCI.md`](UAT-OCI.md) | OCI Distribution | Que el registro habla la Distribution Spec 1.1 con los clientes y con las categorías que el documento nombra, con la versión de cada cliente. |
| [`UAT-OPERATIONS.md`](UAT-OPERATIONS.md) | Recolección y recuperación del almacén de contenido | Que el servicio borra solo los bytes que nadie nombra, solo tras la gracia y con permiso explícito, que explica cada veredicto en una simulación y que no pierde ni repite nada tras un corte. Y que una copia se restaura en una instalación limpia sirviendo los mismos bytes, que el scrub distingue blob corrupto, referencia rota y huérfano, y que ninguna reparación inventa bytes. |
| [`UAT-SECURITY.md`](UAT-SECURITY.md) | Confianza en la distribución | Que un artefacto que nadie ha promovido no se sirve por ningún protocolo (raw u OCI), ni por una etiqueta movida, un manifiesto alternativo, un referrer o un alias, y que revocar tiene efecto en la petición siguiente. |
| [`UAT-PROXY-VIRTUAL.md`](UAT-PROXY-VIRTUAL.md) | Repository proxy remoto | Que un proxy solo sirve lo que verificó contra su upstream (checksum, tamaño, sin truncados) y deja un recibo de procedencia, que el servicio solo sale a los hosts autorizados (sin loopback, redes privadas ni metadatos, ni por redirect ni por rebind) y que la credencial nunca viaja a otro origen. La caché y los repositorios virtuales quedan fuera. |
| [`UAT-FORMATS.md`](UAT-FORMATS.md) | Formatos de paquetes (Maven 2, npm y PyPI: hosted, proxy y group) | Que `mvn deploy` y `mvn dependency:get` publican y resuelven con el layout estándar, con coordenadas validadas contra el POM, releases inmutables, snapshots repuestos, checksums comprobados y `maven-metadata.xml` derivado del catálogo, con `401`/`403` y sobreviviendo a un reinicio. Con proxy y group: un `mvn` resuelve por un group con hosted y proxy, y de nuevo sin red desde lo cacheado; snapshots revalidados por TTL, releases no sobrescritas, prioridad de miembros, checksums del fichero servido y credenciales segregadas. Con npm: el cliente `npm` real publica y instala paquetes con y sin alcance con `integrity` comprobado, mueve dist-tags, depreca, ve rechazado `unpublish` con rastro, e instala por un group con proxy y de nuevo offline sin tocar el upstream. Con PyPI: `twine` publica un wheel y un sdist y `pip` los instala; el hash declarado se comprueba contra los bytes que llegan, el nombre se normaliza (PEP 503), el índice negocia HTML y JSON (PEP 503 y PEP 691) con el mismo hash, las distribuciones son inmutables, una versión se retira entera (PEP 592) y sigue descargable, y el proxy vuelve a pintar el índice de su upstream en vez de devolver su HTML. |

Los informes de cada ejecución--que son lo que se archiva-- no viven aquí:
se escriben en `target/uat/` en el momento de correr la suite, y la
pipeline los archiva con la ejecución.

Lo que el repositorio **sí** tiene, y no es un contrato, está en
[`ARCHITECTURE_STATUS.md`](../ARCHITECTURE_STATUS.md): ese documento dice
qué existe, qué es experimental y qué no existe todavía.
