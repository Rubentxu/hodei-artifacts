use shared_test::dynamic_compose::{generate_unique_compose_file, cleanup_generated_compose_files};

#[tokio::test]
async fn test_dynamic_port_allocation() {
    // Test that we can generate multiple compose files with unique ports
    let output_dir = "/tmp/hodei-test-dynamic-ports";
    let base_compose_path = "/home/rubentxu/Proyectos/rust/hodei-artifacts/tests/compose/docker-compose.yml";
    
    // Generate multiple compose files
    let result1 = generate_unique_compose_file(base_compose_path, output_dir)
        .expect("Failed to generate first compose file");
    
    let result2 = generate_unique_compose_file(base_compose_path, output_dir)
        .expect("Failed to generate second compose file");
    
    let result3 = generate_unique_compose_file(base_compose_path, output_dir)
        .expect("Failed to generate third compose file");
    
    // Verify all files have different paths
    assert_ne!(result1.file_path, result2.file_path);
    assert_ne!(result1.file_path, result3.file_path);
    assert_ne!(result2.file_path, result3.file_path);
    
    // Verify all ports are unique
    let ports1 = result1.ports;
    let ports2 = result2.ports;
    let ports3 = result3.ports;
    
    assert_ne!(ports1.mongo_port, ports2.mongo_port);
    assert_ne!(ports1.mongo_port, ports3.mongo_port);
    assert_ne!(ports2.mongo_port, ports3.mongo_port);
    
    assert_ne!(ports1.kafka_port, ports2.kafka_port);
    assert_ne!(ports1.kafka_port, ports3.kafka_port);
    assert_ne!(ports2.kafka_port, ports3.kafka_port);
    
    assert_ne!(ports1.s3_port, ports2.s3_port);
    assert_ne!(ports1.s3_port, ports3.s3_port);
    assert_ne!(ports2.s3_port, ports3.s3_port);
    
    // Verify ports are within valid range
    assert!(ports1.mongo_port > 1024 && ports1.mongo_port < 65535);
    assert!(ports2.mongo_port > 1024 && ports2.mongo_port < 65535);
    assert!(ports3.mongo_port > 1024 && ports3.mongo_port < 65535);
    
    assert!(ports1.kafka_port > 1024 && ports1.kafka_port < 65535);
    assert!(ports2.kafka_port > 1024 && ports2.kafka_port < 65535);
    assert!(ports3.kafka_port > 1024 && ports3.kafka_port < 65535);
    
    assert!(ports1.s3_port > 1024 && ports1.s3_port < 65535);
    assert!(ports2.s3_port > 1024 && ports2.s3_port < 65535);
    assert!(ports3.s3_port > 1024 && ports3.s3_port < 65535);
    
    println!("Generated compose files:");
    println!("File 1: {} - MongoDB: {}, Kafka: {}, S3: {}", 
        result1.file_path, ports1.mongo_port, ports1.kafka_port, ports1.s3_port);
    println!("File 2: {} - MongoDB: {}, Kafka: {}, S3: {}", 
        result2.file_path, ports2.mongo_port, ports2.kafka_port, ports2.s3_port);
    println!("File 3: {} - MongoDB: {}, Kafka: {}, S3: {}", 
        result3.file_path, ports3.mongo_port, ports3.kafka_port, ports3.s3_port);
    
    // Let's check the content of one file to verify Kafka configuration
    let file_content = std::fs::read_to_string(&result1.file_path)
        .expect("Failed to read generated compose file");
    
    // Verify Kafka advertised listeners was updated
    assert!(file_content.contains(&format!("KAFKA_ADVERTISED_LISTENERS: PLAINTEXT://localhost:{}", ports1.kafka_port)),
        "Kafka advertised listeners not updated correctly");
    
    println!("Kafka configuration verified in generated compose file");
    
    // Don't cleanup immediately so we can inspect the files
    // cleanup_generated_compose_files(output_dir)
    //     .expect("Failed to clean up generated compose files");
    
    println!("Dynamic port allocation test passed!");
}