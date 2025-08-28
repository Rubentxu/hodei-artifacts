use super::{render_compose_template, ComposeTemplateVars};

#[test]
fn test_template_rendering() {
    // Create a simple test template
    let test_template = r#"
services:
  mongodb:
    ports:
      - "{{MONGO_HOST_PORT}}:27017"
  kafka:
    environment:
      KAFKA_ADVERTISED_LISTENERS: PLAINTEXT://localhost:{{KAFKA_HOST_PORT}}
  zookeeper:
    ports:
      - "{{ZOOKEEPER_HOST_PORT}}:2181"
  localstack:
    ports:
      - "{{S3_HOST_PORT}}:4566"
networks:
  {{NETWORK_NAME}}:
    ipam:
      config:
        - subnet: {{SUBNET}}
"#;
    
    // Write test template to temporary file
    let temp_dir = std::env::temp_dir();
    let template_path = temp_dir.join("test-template.yml");
    std::fs::write(&template_path, test_template).unwrap();
    
    // Test variables
    let template_vars = ComposeTemplateVars {
        network_name: "test-network-123".to_string(),
        subnet: "172.16.0.0/16".to_string(),
        mongo_host_port: 27018,
        kafka_host_port: 9093,
        zookeeper_host_port: 2182,
        s3_host_port: 4567,
    };
    
    // Render the template
    let result = render_compose_template(
        template_path.to_str().unwrap(),
        &template_vars.to_hashmap()
    );
    
    assert!(result.is_ok(), "Template rendering failed: {:?}", result.err());
    
    let rendered = result.unwrap();
    
    // Verify all placeholders were replaced
    assert!(rendered.contains("test-network-123"), "Network name not replaced");
    assert!(rendered.contains("172.16.0.0/16"), "Subnet not replaced");
    assert!(rendered.contains("27018"), "Mongo port not replaced");
    assert!(rendered.contains("9093"), "Kafka port not replaced");
    assert!(!rendered.contains("{{"), "Placeholders remain in output");
    assert!(!rendered.contains("}}"), "Placeholders remain in output");
    
    // Clean up
    std::fs::remove_file(template_path).unwrap();
}