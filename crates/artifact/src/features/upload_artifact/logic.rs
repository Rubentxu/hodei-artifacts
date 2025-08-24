//! Lógica pura del feature upload_artifact (separada para testear sin efectos secundarios)
use crate::domain::model::{Artifact};

pub fn compute_post_persist_side_effects(_artifact: &Artifact) {
    // Placeholder: aquí se podrían calcular métricas, derivar eventos internos, etc.
}

