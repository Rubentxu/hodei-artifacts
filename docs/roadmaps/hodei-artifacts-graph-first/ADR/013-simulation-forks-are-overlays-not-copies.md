# ADR-013 — La simulacion es una superposicion sobre el estado real, no una copia

**Estado:** ACEPTADA. Es el mecanismo que acepta el SPIKE-008, y deja los limites de la simulacion declarados.
**Hito:** M7 (SPIKE-008).
**Contexto:** cambiar una politica de promocion y descubrir despues que rompio tres despliegues es un mal negocio. La simulacion responde a "que pasaria si" sin tocar la verdad. La pregunta del SPIKE-008 era **como** se crea ese estado aislado, de forma que la traza y el diff sigan siendo deterministas.
**Decision:** el fork es una **superposicion** sobre el estado real: el estado real se lee y no se toca, y el fork son los hechos hipoteticos, etiquetados como hipoteticos, y nada mas. NO hay copia del grafo, NO hay namespace aislado que reproducir, NO hay rama del almacen.
**Consecuencias:** una simulacion cuesta lo que cuestan sus hechos hipoteticos (pocos) y no lo que cuesta el estado (156 s de proyeccion y 656.6 s de reconstruccion a la escala de referencia). El aislamiento deja de ser una promesa y pasa a ser una construccion: el puerto del grafo **no tiene metodo de escritura**, y el servicio comprueba el numero de relaciones antes y despues de simular. Los hechos hipoteticos llevan una **etiqueta** con el identificador de la simulacion, y por eso las consultas normales del grafo no los ven nunca. El diff se parece al diff de git: que relaciones **aparecen**, que relaciones **desaparecen**, y que **decisiones cambian**, con la version de la politica que las produjo.
**Alternativas:** reproducir desde un checkpoint en un namespace aislado (rechazada: cuesta 156 s por simulacion, y el estado se reproduce entero para preguntar por un sujeto); rama o snapshot nativo del almacen (rechazada: el motor embebido bloquea el directorio y una rama del almacen es una copia de archivos con sus trampas; ademas no es portable); motor de workflows con estado (rechazada: ya se rechazo en el SPIKE-006).
**Gates:** el diff es reproducible: el mismo checkpoint, los mismos hechos hipoteticos y las mismas versiones de politica dan el mismo diff. Los **limites** de la simulacion estan declarados y se comprueban ANTES de simular: cuantos hechos hipoteticos, cuantos sujetos afectados y cuanto vive. El grafo real tiene las mismas relaciones antes y despues, y eso se comprueba, no se promete.

## Los mecanismos que se compararon

| Mecanismo | Coste por simulacion | Aislamiento | Determinismo |
|---|---|---|---|
| Reproducir en un namespace aislado | 156 s (proyeccion) / 656.6 s (reconstruccion) a la escala de referencia | Total, y con un coste enorme | Perfecto |
| Superposicion sobre la base | Lo que cuestan los hechos hipoteticos | Total por construccion | Perfecto |
| Snapshot o rama nativa del almacen | Una copia del directorio | Depende del motor | Perfecto |
| Checkpoint mas superposicion | Lo que cuestan los hechos hipoteticos | Total por construccion | Perfecto |

La superposicion gana en el coste y no pierde en nada de lo demas, y ademas evita el problema de la copia: una copia es un estado mas que alguien tiene que mantener, y un estado mas es un estado que puede quedar viejo.

## Los limites

Una simulacion es un recurso, y un recurso sin limite se come la maquina. Los limites estan **declarados** y se comprueban **antes** de hacer nada:

- **Cuantos hechos hipoteticos**: 100 por defecto.
- **Cuantos sujetos afectados**: 1000 por defecto.
- **Cuanto vive**: 24 horas por defecto.

Y el limite que se escapa es el de la memoria: un fork superpuesto no copia el estado, pero **leer** el estado real de un sujeto con una vecindad enorme cuesta lo que cuesta esa vecindad. Ese limite lo pone el grafo, no la simulacion, y por eso la simulacion no lo esconde: dice cuantos hechos y cuantos sujetos toca.
