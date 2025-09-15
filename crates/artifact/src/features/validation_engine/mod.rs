pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

#[cfg(test)]
mod use_case_test;

// Expose only the public parts of the feature
pub use di::ValidationEngineDIContainer;
