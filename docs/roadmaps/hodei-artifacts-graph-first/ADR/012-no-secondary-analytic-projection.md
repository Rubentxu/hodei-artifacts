# ADR-012 — No hay una proyeccion analitica secundaria: el grafo transaccional y una vista materializada del impacto bastan

**Estado:** ACEPTADA. La decision del SPIKE-003 es "no hace falta", con la medicion que la sostiene.
**Hito:** M5 (SPIKE-003).
**Contexto:** el grafo transaccional (SurrealDB sobre RocksDB) responde el radio de impacto de varios saltos en un p95 de **650-701 ms** a la carga de referencia del SPIKE-012 (107 148 piezas, 331 664 nodos, 211 373 relaciones), y el objetivo acordado era un p95 de **50 ms**. La pregunta era si una proyeccion analitica (LadybugDB) mejoraba materialmente esas consultas como para justificar un segundo almacen.
**Decision:** NO se anade una proyeccion analitica secundaria. El grafo transaccional con sus indices se queda, y la forma de alcanzar el objetivo es una **vista materializada del impacto** dentro del mismo almacen, que la proyeccion escribe cuando llega una vulnerabilidad y que la consulta lee en una fila en vez de ~1044.
**Consecuencias:** el grafo transaccional sigue siendo la verdad historica y no hay que mantener dos copias ni dos procesos. La consulta de varios saltos tiene que llegar al objetivo por la vista materializada, y eso es trabajo pendiente con su propia medicion. La proyeccion analitica no es una opcion para el futuro cercano: su coste de proyeccion medido es de **> 55 minutos y 5 GB** a la escala de referencia, contra los 656.6 s (11 minutos) de reconstruccion del almacen transaccional, y su ventaja de consulta medida con el SDK de Rust es **negativa** (23 ms contra 3 ms en el mismo recorrido).
**Alternativas:** LadybugDB como segunda proyeccion (rechazada por el coste de proyeccion y por la ventaja de consulta medida); solo el indice del grafo (descartada: multiplica por veintidos, pero no quita el suelo de ~1 ms por fila leida); PostgreSQL+TypeDB (rechazada antes, en ADR-001).
**Gates:** el radio de impacto con la vista materializada responde en el p95 de 50 ms a la carga de referencia, y el coste de mantener la vista es menor que el de reconstruir la proyeccion del grafo. LadybugDB no es la verdad historica: la verdad historica es el grafo transaccional, siempre.

## La medicion que la sostiene

La comparacion es del **mismo dataset** y del **mismo recorrido**: el binario de la medicion construye el grafo en SurrealDB, lo mide y lo exporta; el prototipo importa ese mismo dataset en LadybugDB y hace la misma pregunta.

| | grafo transaccional | proyeccion analitica |
|---|---|---|
| radio de impacto p50 (escala pequena) | 2 ms | 18 ms |
| radio de impacto p95 (escala pequena) | 3 ms | 23 ms |
| radio de impacto p95 (carga de referencia) | 650-701 ms | (la importacion no termino en 55 min) |
| linaje p95 | 2-4 ms | n/a |
| coste de la proyeccion a la escala de referencia | 656.6 s (reconstruccion) | > 55 min y 5 GB (importacion) |
| subjects alcanzados | 522 | 522 |

Y el motivo por el que el grafo transaccional llega a 650 ms esta medido tambien: SurrealDB sobre RocksDB devuelve **cada fila a ~1 ms**, y el recorrido tiene que leer ~1044 relaciones. Ni el numero de columnas ni la forma de la consulta lo cambian (502 ms con seis columnas, 454 ms con una).

## Dos cosas que el prototipo tambien dijo

1. **Los dos motores no conviven en el mismo binario.** La libreria precompilada de LadybugDB trae su propio zstd y choca con el de SurrealDB: hay que separarlos en procesos distintos. Una proyeccion secundaria es un proceso aparte, y el prototipo tiene que parecerse a lo que seria.
2. **El motor analitico necesita OpenSSL a mano.** Su cliente HTTP con TLS viene compilado con TLS y el enlazador no lo resuelve solo: hay que pedir `-lssl -lcrypto` explicitamente. No es un impedimento, pero es un peldaño que el producto tendria que subir desde el primer dia.
