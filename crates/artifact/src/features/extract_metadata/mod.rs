pub mod error;
pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod event_handler;
pub mod di;
pub mod mocks;


// Expose only the public parts of the feature.
pub use di::ExtractMetadataDIContainer;
pub use event_handler::PackageVersionPublishedEventHandler;
pub use api::ExtractMetadataApi;