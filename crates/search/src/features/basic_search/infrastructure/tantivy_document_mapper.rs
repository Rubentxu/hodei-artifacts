use tantivy::{doc, schema::Field, schema::Value};
use crate::features::basic_search::dto::ArtifactDocument;

pub struct TantivyDocumentMapper {
    id_field: Field,
    name_field: Field,
    version_field: Field,
    package_type_field: Field,
    repository_field: Field,
}

impl TantivyDocumentMapper {
    pub fn new(
        id_field: Field,
        name_field: Field,
        version_field: Field,
        package_type_field: Field,
        repository_field: Field,
    ) -> Self {
        Self {
            id_field,
            name_field,
            version_field,
            package_type_field,
            repository_field,
        }
    }
    
    pub fn to_document(&self, artifact: &ArtifactDocument) -> tantivy::TantivyDocument {
        doc! {
            self.id_field => artifact.id.clone(),
            self.name_field => artifact.name.to_lowercase(),
            self.version_field => artifact.version.clone(),
            self.package_type_field => artifact.package_type.clone(),
            self.repository_field => artifact.repository.clone(),
        }
    }
    
    pub fn from_document(&self, doc: &tantivy::TantivyDocument) -> Option<ArtifactDocument> {
        let id = doc.get_first(self.id_field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?;
            
        let name = doc.get_first(self.name_field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?;
            
        let version = doc.get_first(self.version_field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?;
            
        let package_type = doc.get_first(self.package_type_field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?;
            
        let repository = doc.get_first(self.repository_field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?;
            
        Some(ArtifactDocument {
            id,
            name,
            version,
            package_type,
            repository,
        })
    }
}