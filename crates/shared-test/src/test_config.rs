//! Test configuration system with environment variables for dynamic ports
//!
//! This module provides a configuration system that can handle dynamic ports
//! and URLs for parallel test execution using environment variables.

use std::env;

/// Test environment configuration
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub mongo_uri: String,
    pub kafka_brokers: String,
    pub s3_endpoint: String,
    pub compose_file_path: Option<String>,
}

impl TestConfig {
    /// Create test configuration from environment variables
    /// If environment variables are not set, uses default values
    pub fn from_env() -> Self {
        let mongo_port = env::var("TEST_MONGO_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(27017);
            
        let kafka_port = env::var("TEST_KAFKA_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(9092);
            
        let s3_port = env::var("TEST_S3_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4566);
        
        let compose_file_path = env::var("TEST_COMPOSE_FILE").ok();
        
        Self {
            mongo_uri: format!("mongodb://127.0.0.1:{}", mongo_port),
            kafka_brokers: format!("127.0.0.1:{}", kafka_port),
            s3_endpoint: format!("http://127.0.0.1:{}", s3_port),
            compose_file_path,
        }
    }
    
    /// Create test configuration with specific ports
    pub fn with_ports(mongo_port: u16, kafka_port: u16, s3_port: u16) -> Self {
        Self {
            mongo_uri: format!("mongodb://127.0.0.1:{}", mongo_port),
            kafka_brokers: format!("127.0.0.1:{}", kafka_port),
            s3_endpoint: format!("http://127.0.0.1:{}", s3_port),
            compose_file_path: None,
        }
    }
    
    /// Create test configuration from dynamic ports
    pub fn from_dynamic_ports(ports: &super::dynamic_compose::DynamicPorts) -> Self {
        Self::with_ports(ports.mongo_port, ports.kafka_port, ports.s3_port)
    }
}

/// Set environment variables for test configuration
pub fn set_test_env_vars(ports: &super::dynamic_compose::DynamicPorts) {
    env::set_var("TEST_MONGO_PORT", ports.mongo_port.to_string());
    env::set_var("TEST_KAFKA_PORT", ports.kafka_port.to_string());
    env::set_var("TEST_S3_PORT", ports.s3_port.to_string());
}

/// Clear test environment variables
pub fn clear_test_env_vars() {
    env::remove_var("TEST_MONGO_PORT");
    env::remove_var("TEST_KAFKA_PORT");
    env::remove_var("TEST_S3_PORT");
    env::remove_var("TEST_COMPOSE_FILE");
}