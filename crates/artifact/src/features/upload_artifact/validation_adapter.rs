use async_trait::async_trait;
use bytes::Bytes;
use tracing::{debug, warn};

use super::{dto::UploadArtifactCommand, ports::ArtifactValidator};

/// Validador de artefactos que implementa reglas de validación reales
pub struct ValidationEngineArtifactValidator {
    max_file_size: usize,
    allowed_mime_types: Vec<String>,
    blocked_extensions: Vec<String>,
}

impl ValidationEngineArtifactValidator {
    pub fn new() -> Self {
        Self {
            max_file_size: 100 * 1024 * 1024, // 100MB por defecto
            allowed_mime_types: vec![
                "application/java-archive".to_string(),
                "application/zip".to_string(),
                "application/gzip".to_string(),
                "application/xml".to_string(),
                "application/json".to_string(),
                "application/javascript".to_string(),
                "text/x-python".to_string(),
                "text/plain".to_string(),
                "application/octet-stream".to_string(),
            ],
            blocked_extensions: vec![
                ".exe".to_string(),
                ".dll".to_string(),
                ".bat".to_string(),
                ".sh".to_string(),
                ".py".to_string(),
                ".rb".to_string(),
            ],
        }
    }

    pub fn with_max_file_size(mut self, size: usize) -> Self {
        self.max_file_size = size;
        self
    }

    pub fn with_allowed_mime_types(mut self, types: Vec<String>) -> Self {
        self.allowed_mime_types = types;
        self
    }

    pub fn with_blocked_extensions(mut self, extensions: Vec<String>) -> Self {
        self.blocked_extensions = extensions;
        self
    }

    /// Valida el tamaño del archivo
    fn validate_file_size(&self, content: &Bytes) -> Result<(), String> {
        let size = content.len();
        if size > self.max_file_size {
            return Err(format!(
                "File size {} bytes exceeds maximum allowed size of {} bytes",
                size, self.max_file_size
            ));
        }
        debug!("File size validation passed: {} bytes", size);
        Ok(())
    }

    /// Valida la extensión del archivo
    fn validate_file_extension(&self, file_name: &str) -> Result<(), String> {
        let file_name_lower = file_name.to_lowercase();

        for blocked_ext in &self.blocked_extensions {
            if file_name_lower.ends_with(blocked_ext) {
                return Err(format!(
                    "File extension '{}' is not allowed for security reasons",
                    blocked_ext
                ));
            }
        }

        debug!("File extension validation passed for: {}", file_name);
        Ok(())
    }

    /// Valida el contenido básico del archivo
    fn validate_file_content(&self, content: &Bytes, file_name: &str) -> Result<(), String> {
        // Validación básica de contenido
        if content.is_empty() {
            return Err("File content is empty".to_string());
        }

        // Validación de magic numbers para detectar tipos de archivo
        if content.len() >= 4 {
            let header = &content[..4];

            // Detectar archivos ejecutables (MZ header)
            if header == b"MZ\x90\x00" {
                return Err("Executable files are not allowed".to_string());
            }

            // Detectar scripts potencialmente peligrosos
            if file_name.ends_with(".js") || file_name.ends_with(".py") {
                let content_str = String::from_utf8_lossy(&content[..content.len().min(1024)]);
                if content_str.contains("eval(") || content_str.contains("exec(") {
                    warn!("Potentially dangerous code detected in {}", file_name);
                    return Err("Potentially dangerous code detected".to_string());
                }
            }
        }

        debug!("File content validation passed for: {}", file_name);
        Ok(())
    }

    /// Valida las coordenadas del paquete
    fn validate_package_coordinates(&self, command: &UploadArtifactCommand) -> Result<(), String> {
        // Validar nombre del paquete
        if command.coordinates.name.is_empty() {
            return Err("Package name cannot be empty".to_string());
        }

        if command.coordinates.name.len() > 100 {
            return Err("Package name is too long (max 100 characters)".to_string());
        }

        // Validar versión
        if command.coordinates.version.is_empty() {
            return Err("Package version cannot be empty".to_string());
        }

        if !command
            .coordinates
            .version
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '+' || c == '_')
        {
            return Err("Package version contains invalid characters".to_string());
        }

        debug!("Package coordinates validation passed");
        Ok(())
    }

    /// Valida metadatos del comando
    fn validate_command_metadata(&self, command: &UploadArtifactCommand) -> Result<(), String> {
        // Validar longitud del nombre de archivo
        if command.file_name.len() > 255 {
            return Err("File name is too long (max 255 characters)".to_string());
        }

        // Validar que el nombre de archivo no contenga caracteres peligrosos
        if command.file_name.contains("..")
            || command.file_name.contains('/')
            || command.file_name.contains('\\')
        {
            return Err("File name contains invalid characters".to_string());
        }

        // Validar tamaño de contenido
        if command.content_length == 0 {
            return Err("Content length cannot be zero".to_string());
        }

        if command.content_length > self.max_file_size as u64 {
            return Err(format!(
                "Content length {} exceeds maximum allowed size of {} bytes",
                command.content_length, self.max_file_size
            ));
        }

        debug!("Command metadata validation passed");
        Ok(())
    }
}

#[async_trait]
impl ArtifactValidator for ValidationEngineArtifactValidator {
    async fn validate(
        &self,
        command: &UploadArtifactCommand,
        content: &Bytes,
    ) -> Result<(), Vec<String>> {
        debug!(
            "Starting artifact validation for file: {}",
            command.file_name
        );

        let mut errors = Vec::new();

        // 1. Validar metadatos del comando
        if let Err(e) = self.validate_command_metadata(command) {
            errors.push(e);
        }

        // 2. Validar coordenadas del paquete
        if let Err(e) = self.validate_package_coordinates(command) {
            errors.push(e);
        }

        // 3. Validar extensión del archivo
        if let Err(e) = self.validate_file_extension(&command.file_name) {
            errors.push(e);
        }

        // 4. Validar tamaño del archivo
        if let Err(e) = self.validate_file_size(content) {
            errors.push(e);
        }

        // 5. Validar contenido del archivo
        if let Err(e) = self.validate_file_content(content, &command.file_name) {
            errors.push(e);
        }

        if errors.is_empty() {
            debug!("Artifact validation completed successfully");
            Ok(())
        } else {
            warn!("Artifact validation failed with {} errors", errors.len());
            Err(errors)
        }
    }
}
