// Search Crate

pub mod features {
    pub mod basic_search;
    pub mod index_text_documents;
    pub mod search_full_text;
}

pub use features::*;
pub use features::index_text_documents::*;
pub use features::search_full_text::*;