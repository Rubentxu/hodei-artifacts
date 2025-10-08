pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;
#[cfg(test)]
pub mod use_case_test;

// Re-export use case for external consumption
pub use use_case::BuildSchemaUseCase;

// Re-export public bundle factory for external crates
pub use di::create_schema_registration_components;
