#[cfg(test)]
mod tests {
    use hodei_artifacts_api::config::{Config, ConfigError};
    
    #[test]
    fn test_config_from_env() {
        // This test would verify that the Config::from_env() function works correctly
        // We might need to set some environment variables for this test
        std::env::set_var("SERVER_HOST", "127.0.0.1");
        std::env::set_var("SERVER_PORT", "8080");
        std::env::set_var("LOG_LEVEL", "debug");
        std::env::set_var("LOG_FORMAT", "json");
        
        let config = Config::from_env();
        assert!(config.is_ok());
        
        let config = config.unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert!(config.metrics.enabled);
    }
    
    #[test]
    fn test_config_defaults() {
        // Remove any existing environment variables to test defaults
        std::env::remove_var("SERVER_HOST");
        std::env::remove_var("SERVER_PORT");
        std::env::remove_var("LOG_LEVEL");
        std::env::remove_var("LOG_FORMAT");
        
        let config = Config::from_env();
        assert!(config.is_ok());
        
        let config = config.unwrap();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.logging.level, "info,policy_baas_mvp=debug");
    }
    
    #[test]
    fn test_config_invalid_port() {
        std::env::set_var("SERVER_PORT", "invalid");
        
        let config = Config::from_env();
        assert!(config.is_err());
        
        match config.unwrap_err() {
            ConfigError::InvalidPort => (),
            _ => panic!("Expected InvalidPort error"),
        }
        
        // Clean up
        std::env::remove_var("SERVER_PORT");
    }
}
