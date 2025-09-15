pub mod adapter;
pub mod api;
pub mod di;
pub mod dto;
pub mod error;
pub mod event_handler;
pub mod mocks;
pub mod ports;
pub mod use_case;

// Expose only the public parts of the feature.
pub use api::ExtractMetadataApi;
pub use di::ExtractMetadataDIContainer;
pub use event_handler::PackageVersionPublishedEventHandler;
