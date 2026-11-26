//! Infrastructure implementations for hodei-iam

pub mod dgraph;
pub mod dgraph_schema;
pub mod hrn_generator;
// HSP-213: el adaptador sobre el motor embebido vive tras la feature
// `legacy`; el binario productivo no la declara.
#[cfg(feature = "legacy")]
pub mod surreal;

pub use dgraph::DgraphIamAdapter;
pub use dgraph_schema::DgraphSchemaStore;
