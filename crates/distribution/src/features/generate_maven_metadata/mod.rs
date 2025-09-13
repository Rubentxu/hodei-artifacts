// crates/distribution/src/features/generate_maven_metadata/mod.rs

//! Feature para generar metadata XML de Maven
//! 
//! Esta feature implementa la generación dinámica de archivos maven-metadata.xml
//! siguiendo el estándar de Maven Repository con soporte para:
//! 
//! - Generación de metadata para grupos, artefactos y versiones
//! - Caché de metadata con TTL configurable
//! - Actualización incremental de metadata
//! - Validación de coordenadas Maven
//! - Soporte para snapshots y releases

// Exportar DTOs
pub use dto::{
    GenerateMavenMetadataRequest, GenerateMavenMetadataResponse, MavenMetadataDto,
    MavenMetadataVersioningDto, MavenMetadataSnapshotDto, MavenMetadataSnapshotVersionDto,
};

// Exportar puertos (interfaces)
pub use ports::{
    MavenMetadataGenerator, MavenArtifactLister, MavenMetadataCache,
    MetadataGeneratorError, ArtifactListerError, MetadataCacheError,
};

// Exportar caso de uso
pub use use_case::GenerateMavenMetadataUseCase;

// Exportar API
pub use api::GenerateMavenMetadataApi;

// Exportar contenedor de dependencias
pub use di::GenerateMavenMetadataDIContainer;

// Módulos internos
pub mod dto;
pub mod ports;
pub mod use_case;
pub mod api;
pub mod adapter;
pub mod di;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::maven::{MavenCoordinates, MavenVersion};
    
    #[test]
    fn test_feature_exports() {
        // Verificar que todos los tipos principales están disponibles
        let _request = GenerateMavenMetadataRequest {
            coordinates: MavenCoordinates::new("com.example", "test", "1.0.0").unwrap(),
            repository_id: "test-repo".to_string(),
            force_regenerate: false,
        };
        
        // Si compilamos sin errores, los exports están correctos
        assert!(true);
    }
}