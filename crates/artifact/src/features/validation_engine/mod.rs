pub mod error;
pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

#[cfg(test)]
mod use_case_test;
#[cfg(test)]
mod api_test;

// Expose only the public parts of the feature
pub use di::ValidationEngineDIContainer;
pub use api::ValidationEngineApi;