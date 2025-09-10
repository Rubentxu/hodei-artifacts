use tantivy::schema::*;

#[derive(Debug, Clone)]
pub struct SearchField {
    pub id: Field,
    pub name: Field,
    pub version: Field,
    pub package_type: Field,
    pub repository: Field,
    pub description: Field,
    pub tags: Field,
}

#[derive(Debug, Clone)]
pub struct SearchSchema {
    pub schema: Schema,
    pub fields: SearchField,
}

impl SearchSchema {
    pub fn new() -> Self {
        let mut schema_builder = Schema::builder();
        
        // Create fields with appropriate types and options
        let id = schema_builder.add_text_field("id", STRING | STORED);
        let name = schema_builder.add_text_field("name", TEXT | STORED);
        let version = schema_builder.add_text_field("version", STRING | STORED);
        let package_type = schema_builder.add_text_field("package_type", STRING | STORED);
        let repository = schema_builder.add_text_field("repository", STRING | STORED);
        let description = schema_builder.add_text_field("description", TEXT | STORED);
        let tags = schema_builder.add_text_field("tags", STRING | STORED);
        
        let schema = schema_builder.build();
        
        let fields = SearchField {
            id,
            name,
            version,
            package_type,
            repository,
            description,
            tags,
        };
        
        Self { schema, fields }
    }
    
    pub fn id_field(&self) -> Field {
        self.fields.id
    }
    
    pub fn name_field(&self) -> Field {
        self.fields.name
    }
    
    pub fn version_field(&self) -> Field {
        self.fields.version
    }
    
    pub fn package_type_field(&self) -> Field {
        self.fields.package_type
    }
    
    pub fn repository_field(&self) -> Field {
        self.fields.repository
    }
    
    pub fn description_field(&self) -> Field {
        self.fields.description
    }
    
    pub fn tags_field(&self) -> Field {
        self.fields.tags
    }
}

impl Default for SearchSchema {
    fn default() -> Self {
        Self::new()
    }
}