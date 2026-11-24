//! Infrastructure implementations for hodei-iam

pub mod dgraph;
pub mod hrn_generator;
pub mod surreal;

pub use dgraph::DgraphIamAdapter;
