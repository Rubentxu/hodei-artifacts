//! La simulacion: preguntar "que pasaria si" sin tocar la verdad.
//!
//! Cambiar una politica de promocion y descubrir despues que rompio tres
//! despliegues es un mal negocio. Aqui se responde a esa pregunta sin
//! desplegar nada: un **fork** superpuesto al estado real, y un **diff**
//! que dice que relaciones aparecen, que relaciones desaparecen y que
//! decisiones cambian.
//!
//! # La decision del SPIKE-008: superposicion, no copia
//!
//! El SPIKE-008 comparaba cuatro mecanismos:
//!
//! 1. reproducir desde un checkpoint en un namespace aislado;
//! 2. una superposicion copy-on-write sobre la proyeccion base;
//! 3. un snapshot o una rama nativa del almacen;
//! 4. un checkpoint mas una superposicion.
//!
//! **Se elige la superposicion (2 y 4)**, y la razon es una cuenta:
//!
//! - Una **copia** cuesta lo que cuesta el estado: a la escala de
//!   referencia, la proyeccion tarda 156 s y la reconstruccion 656.6 s. Y
//!   eso es por simulacion, cada vez que alguien pregunta.
//! - Una **superposicion** cuesta lo que cuestan los hechos hipoteticos,
//!   que son pocos: el fork es un VALOR, no una copia.
//! - El **almacen no tiene ramas portables**: el motor embebido bloquea
//!   el directorio, y una rama del almacen seria una copia de archivos
//!   con las suas trampas.
//!
//! Y el aislamiento no es una promesa, es una **construccion**: el puerto
//! del grafo **no tiene un metodo de escritura**, y el servicio comprueba
//! el numero de relaciones antes y despues de simular.
//!
//! # Lo que el diff dice
//!
//! El diff se parece al diff de git: se ve que relaciones **aparecen** y
//! que relaciones **desaparecen**, y que **decisiones cambian**, con la
//! version de la politica que las produjo. Y una decision que sigue igual
//! no es un cambio: un diff lleno de ruido no es un diff.
//!
//! # Los limites
//!
//! Una simulacion es un recurso, y un recurso sin limite se come la
//! maquina. Los limites son **declarados** --cuantos hechos, cuantos
//! sujetos, cuanto vive-- y se comprueban **antes** de hacer nada.

#![forbid(unsafe_code)]

pub mod base;
pub mod diff;
pub mod fork;
pub mod hypothetical;
pub mod ports;
pub mod service;

pub use base::{SimulationBase, the_seal_of};
pub use diff::{DecisionChange, SimulationDiff, StructuralDiff, TheDecision};
pub use fork::{Checkpoint, SimulationFork, SimulationLimits};
pub use hypothetical::{Hypothetical, HypotheticalClaim, RelationFact, SimulationTag};
pub use ports::{DecisionEvaluator, SimulationGraph};
pub use service::{SimulationError, SimulationService};
