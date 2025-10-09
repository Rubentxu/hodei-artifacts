pub mod dto;
pub mod error;
pub mod factories;
pub mod ports;
pub mod use_case;
#[cfg(test)]
pub mod use_case_test;

pub use ports::RegisterEntityTypePort;

// Re-export use case for external consumption
pub use use_case::RegisterEntityTypeUseCase;
