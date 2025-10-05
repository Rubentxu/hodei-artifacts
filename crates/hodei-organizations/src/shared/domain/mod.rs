// Módulos de dominio - INTERNOS al crate
pub(crate) mod account;
pub(crate) mod ou;
pub(crate) mod scp;

// Módulo de eventos - PÚBLICO para suscriptores externos
pub mod events;

// HRN helper para uso interno
pub(crate) mod hrn {
    // Import y helper solo disponibles en tests (evita unused_imports y unexpected cfg)
    #[cfg(test)]
    pub(crate) use kernel::Hrn;

    // Compat helper para tests legacy que usaban Hrn::generate("ou")
    #[cfg(test)]
    pub(crate) fn generate(resource_type: &str) -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            resource_type.to_string(),
            format!("{}-gen", resource_type),
        )
    }
}

// ❌ NO exportar entidades públicamente - solo accesibles dentro del crate
// Las entidades se usan internamente en los casos de uso
// Los casos de uso devuelven DTOs, NO entidades
pub(crate) use account::Account;
pub(crate) use ou::OrganizationalUnit;
pub(crate) use scp::ServiceControlPolicy;

// Re-exportar eventos para conveniencia
pub use events::*;

#[cfg(test)]
mod tests {
    use super::hrn;

    #[test]
    fn hrn_generate_helper_produces_expected_suffix() {
        // When
        let generated = hrn::generate("ou");
        let s = generated.to_string();
        // Then
        assert!(
            s.contains("ou-gen"),
            "Expected generated HRN string to contain 'ou-gen', got {s}"
        );
    }
}
