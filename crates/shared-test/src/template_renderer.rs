//! Template renderer for Docker Compose files with placeholder support

use std::collections::HashMap;
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("Failed to read template file: {0}")]
    ReadError(String),
    
    #[error("Missing required placeholder: {0}")]
    MissingPlaceholder(String),
    
    #[error("Template validation failed: {0}")]
    ValidationError(String),
}

/// Renders a Docker Compose template with the provided variables
pub fn render_compose_template(
    template_path: &str,
    variables: &HashMap<&str, String>,
) -> Result<String, TemplateError> {
    // Read the template file
    let template_content = fs::read_to_string(template_path)
        .map_err(|e| TemplateError::ReadError(e.to_string()))?;
    
    // Render the template
    let mut rendered = template_content.clone();
    
    for (key, value) in variables {
        let placeholder = format!("{{{{{}}}}}", key);
        if !rendered.contains(&placeholder) {
            return Err(TemplateError::ValidationError(
                format!("Placeholder {} not found in template", key)
            ));
        }
        rendered = rendered.replace(&placeholder, value);
    }
    
    // Validate that all placeholders were replaced
    validate_no_placeholders(&rendered)?;
    
    // Validate YAML syntax (basic check)
    validate_yaml_syntax(&rendered)?;
    
    Ok(rendered)
}

/// Validates that no placeholders remain in the rendered content
fn validate_no_placeholders(content: &str) -> Result<(), TemplateError> {
    if content.contains("{{") && content.contains("}}") {
        // Find the first unmatched placeholder
        let start = content.find("{{").unwrap();
        let end = content.find("}}").unwrap() + 2;
        let placeholder = &content[start..end];
        
        return Err(TemplateError::MissingPlaceholder(
            placeholder.to_string()
        ));
    }
    
    Ok(())
}

/// Basic YAML syntax validation
fn validate_yaml_syntax(content: &str) -> Result<(), TemplateError> {
    // Check for basic YAML structure
    if !content.contains("services:") {
        return Err(TemplateError::ValidationError(
            "Missing 'services' section in YAML".to_string()
        ));
    }
    
    if !content.contains("networks:") {
        return Err(TemplateError::ValidationError(
            "Missing 'networks' section in YAML".to_string()
        ));
    }
    
    Ok(())
}

/// Template variables for Docker Compose generation
#[derive(Debug, Clone)]
pub struct ComposeTemplateVars {
    pub network_name: String,
    pub subnet: String,
    pub mongo_host_port: u16,
    pub kafka_host_port: u16,
    pub zookeeper_host_port: u16,
    pub s3_host_port: u16,
}

impl ComposeTemplateVars {
    /// Convert to HashMap for template rendering
    pub fn to_hashmap(&self) -> HashMap<&str, String> {
        let mut vars = HashMap::new();
        vars.insert("NETWORK_NAME", self.network_name.clone());
        vars.insert("SUBNET", self.subnet.clone());
        vars.insert("MONGO_HOST_PORT", self.mongo_host_port.to_string());
        vars.insert("KAFKA_HOST_PORT", self.kafka_host_port.to_string());
        vars.insert("ZOOKEEPER_HOST_PORT", self.zookeeper_host_port.to_string());
        vars.insert("S3_HOST_PORT", self.s3_host_port.to_string());
        vars
    }
}

#[cfg(test)]
mod template_renderer_test;