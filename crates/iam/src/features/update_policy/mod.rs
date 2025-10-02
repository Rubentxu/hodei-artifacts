// crates/iam/src/features/update_policy/mod.rs

pub mod adapter;
pub mod api;
pub mod di;
pub mod dto;
pub mod ports;
pub mod use_case;

#[cfg(test)]
mod adapter_test;
#[cfg(test)]
mod use_case_test;
