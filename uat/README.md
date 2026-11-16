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
| [`UAT-OPERATIONS.md`](UAT-OPERATIONS.md) | Recolección del almacén de contenido | Que el servicio borra solo los bytes que nadie nombra, solo tras la gracia y con permiso explícito, que explica cada veredicto en una simulación y que no pierde ni repite nada tras un corte. |

Los informes de cada ejecución--que son lo que se archiva-- no viven aquí:
se escriben en `target/uat/` en el momento de correr la suite, y la
pipeline los archiva con la ejecución.

Lo que el repositorio **sí** tiene, y no es un contrato, está en
[`ARCHITECTURE_STATUS.md`](../ARCHITECTURE_STATUS.md): ese documento dice
qué existe, qué es experimental y qué no existe todavía.
