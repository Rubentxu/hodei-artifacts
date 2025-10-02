/// Application layer for hodei-iam

pub mod ports;
mod di_configurator;

pub use di_configurator::configure_default_iam_entities;
