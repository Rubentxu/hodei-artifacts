pub mod tantivy_index;
pub mod tantivy_schema;
pub mod tantivy_document_mapper;

// Internal-only: Do not re-export infrastructure types. These are used by the
// feature's adapters and should not leak outside the feature boundary.