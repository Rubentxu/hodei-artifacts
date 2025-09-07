use super::generate_unique_compose_file;

#[test]
fn test_generate_unique_compose_file() {
    // Create a test template
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
    let template_path = temp_dir.join("test-compose-template.yml");
    std::fs::write(&template_path, test_template).unwrap();
    
    // Test the generation function
    let result = generate_unique_compose_file(
        template_path.to_str().unwrap(),
        temp_dir.to_str().unwrap()
    );
    
    assert!(result.is_ok(), "Compose generation failed: {:?}", result.err());
    
    let generation_result = result.unwrap();
    
    // Verify the file was created
    assert!(std::path::Path::new(&generation_result.file_path).exists(), "Compose file not created");
    
    // Verify ports are within valid range
    assert!(generation_result.ports.mongo_port > 1024, "Mongo port too low");
    assert!(generation_result.ports.mongo_port < 65535, "Mongo port too high");
    assert!(generation_result.ports.kafka_port > 1024, "Kafka port too low");
    assert!(generation_result.ports.kafka_port < 65535, "Kafka port too high");
    assert!(generation_result.ports.s3_port > 1024, "S3 port too low");
    assert!(generation_result.ports.s3_port < 65535, "S3 port too high");
    assert!(generation_result.ports.zookeeper_port > 1024, "Zookeeper port too low");
    assert!(generation_result.ports.zookeeper_port < 65535, "Zookeeper port too high");
    
    // Clean up
    std::fs::remove_file(template_path).unwrap();
    std::fs::remove_file(generation_result.file_path).unwrap();
}