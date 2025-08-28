use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use portpicker::pick_unused_port;
use uuid::Uuid;

use crate::template_renderer::{render_compose_template, ComposeTemplateVars};

/// Port assignments for dynamically generated Docker Compose files
#[derive(Debug, Clone)]
pub struct DynamicPorts {
    pub mongo_port: u16,
    pub kafka_port: u16,
    pub s3_port: u16,
    pub zookeeper_port: u16,
}

/// Result type for generate_unique_compose_file
pub struct ComposeGenerationResult {
    pub file_path: String,
    pub ports: DynamicPorts,
}

/// Generates a unique Docker Compose file using template rendering
/// with unique network name, subnet, and dynamic ports to prevent conflicts
pub fn generate_unique_compose_file(
    template_path: &str,
    output_dir: &str,
) -> Result<ComposeGenerationResult, String> {
    // Generate unique identifiers
    let uuid_str = Uuid::new_v4().to_string();
    let unique_network_name = format!("hodei-test-network-{}", &uuid_str[..8]);
    
    // Generate a unique subnet to avoid IP address conflicts
    let uuid_hash = uuid_str.as_bytes().iter().fold(0u32, |acc, &b| acc.wrapping_add(b as u32));
    let subnet_third_octet = (uuid_hash % 254) as u16 + 1; // 1-254
    let unique_subnet = format!("172.{}.0.0/16", subnet_third_octet);
    
    // Generate dynamic ports for all services
    let mongo_port = pick_unused_port().expect("Failed to find unused port for MongoDB");
    let kafka_port = pick_unused_port().expect("Failed to find unused port for Kafka");
    let s3_port = pick_unused_port().expect("Failed to find unused port for LocalStack");
    let zookeeper_port = pick_unused_port().expect("Failed to find unused port for Zookeeper");
    
    // Prepare template variables
    let template_vars = ComposeTemplateVars {
        network_name: unique_network_name,
        subnet: unique_subnet,
        mongo_host_port: mongo_port,
        kafka_host_port: kafka_port,
        zookeeper_host_port: zookeeper_port,
        s3_host_port: s3_port,
    };
    
    // Render the template
    let rendered_content = render_compose_template(template_path, &template_vars.to_hashmap())
        .map_err(|e| format!("Failed to render Docker Compose template: {}", e))?;
    
    // Create output directory if it doesn't exist
    fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;
    
    // Generate output file path
    let file_uuid_str = Uuid::new_v4().to_string();
    let output_filename = format!("docker-compose-{}.yml", &file_uuid_str[..8]);
    let output_path = Path::new(output_dir).join(&output_filename);
    
    // Write the rendered content to the new file
    let mut file = File::create(&output_path)
        .map_err(|e| format!("Failed to create output compose file: {}", e))?;
    
    file.write_all(rendered_content.as_bytes())
        .map_err(|e| format!("Failed to write to output compose file: {}", e))?;
    
    // Return the result with file path and port information
    Ok(ComposeGenerationResult {
        file_path: output_path.to_string_lossy().into_owned(),
        ports: DynamicPorts {
            mongo_port,
            kafka_port,
            s3_port,
            zookeeper_port,
        },
    })
}

/// Cleans up generated Docker Compose files
pub fn cleanup_generated_compose_files(output_dir: &str) -> Result<(), String> {
    if let Ok(entries) = fs::read_dir(output_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() && path.extension().map(|e| e == "yml").unwrap_or(false) {
                    if let Some(file_name) = path.file_name() {
                        if file_name.to_string_lossy().starts_with("docker-compose-") {
                            fs::remove_file(&path)
                                .map_err(|e| format!("Failed to remove generated compose file: {}", e))?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod dynamic_compose_test;