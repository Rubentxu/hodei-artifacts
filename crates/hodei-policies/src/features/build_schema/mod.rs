pub mod dto;
pub mod error;
pub mod factories;
pub mod ports;
pub mod use_case;
#[cfg(test)]
pub mod use_case_test;

pub use ports::BuildSchemaPort;

// Re-export use case for external consumption
pub use use_case::BuildSchemaUseCase;

// Re-export public bundle factory for external crates
pub use factories::create_schema_registration_components;
