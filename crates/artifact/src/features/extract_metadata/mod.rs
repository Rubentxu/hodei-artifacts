pub mod error;
pub mod dto;
pub mod ports;
pub mod maven_parser;
pub mod npm_parser;
pub mod use_case;
pub mod adapter;
pub mod event_handler;
pub mod di;

#[cfg(test)]
mod maven_parser_test;
#[cfg(test)]
mod npm_parser_test;

// Expose only the public parts of the feature.
pub use di::ExtractMetadataDIContainer;
pub use event_handler::PackageVersionPublishedEventHandler;