//! Configuration for distribution infrastructure

use std::env;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct DistributionConfig {
    pub s3_bucket: String,
    pub s3_region: String,
    pub mongodb_uri: String,
    pub mongodb_database: String,
    pub redis_url: String,
    pub cedar_policy_store: String,
    pub max_file_size: usize,
    pub upload_timeout: Duration,
    pub download_timeout: Duration,
}

impl DistributionConfig {
    pub fn from_env() -> Self {
        Self {
            s3_bucket: env::var("S3_BUCKET").unwrap_or_else(|_| "hodei-artifacts".to_string()),
            s3_region: env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            mongodb_uri: env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://localhost:27017".to_string()),
            mongodb_database: env::var("MONGODB_DATABASE").unwrap_or_else(|_| "hodei".to_string()),
            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            cedar_policy_store: env::var("CEDAR_POLICY_STORE").unwrap_or_else(|_| "hodei-policies".to_string()),
            max_file_size: env::var("MAX_FILE_SIZE")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .unwrap_or(100), // MB
            upload_timeout: Duration::from_secs(
                env::var("UPLOAD_TIMEOUT")
                    .unwrap_or_else(|_| "300".to_string())
                    .parse()
                    .unwrap_or(300),
            ),
            download_timeout: Duration::from_secs(
                env::var("DOWNLOAD_TIMEOUT")
                    .unwrap_or_else(|_| "60".to_string())
                    .parse()
                    .unwrap_or(60),
            ),
        }
    }

    pub fn for_testing() -> Self {
        Self {
            s3_bucket: "test-bucket".to_string(),
            s3_region: "us-east-1".to_string(),
            mongodb_uri: "mongodb://localhost:27017".to_string(),
            mongodb_database: "test_hodei".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            cedar_policy_store: "test-policies".to_string(),
            max_file_size: 10, // 10MB for testing
            upload_timeout: Duration::from_secs(30),
            download_timeout: Duration::from_secs(10),
        }
    }
}