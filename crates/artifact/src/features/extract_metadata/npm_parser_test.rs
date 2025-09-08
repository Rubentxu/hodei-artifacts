#[cfg(test)]
mod tests {
    use super::super::npm_parser::NpmParser;
    use super::super::error::MetadataError;

    #[test]
    fn test_parse_simple_package_json() {
        let json_content = r#"{
  "name": "my-package",
  "version": "1.0.0",
  "description": "A sample NPM package",
  "license": "MIT",
  "dependencies": {
    "express": "^4.17.1",
    "lodash": "^4.17.21"
  },
  "devDependencies": {
    "jest": "^27.0.0"
  }
}"#;

        let parser = NpmParser::new();
        let result = parser.parse(json_content);
        
        assert!(result.is_ok());
        
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "my-package");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.description, Some("A sample NPM package".to_string()));
        assert_eq!(metadata.licenses.len(), 1);
        assert_eq!(metadata.licenses[0], "MIT");
        assert_eq!(metadata.dependencies.len(), 3);
        
        // Check regular dependencies
        let express_dep = metadata.dependencies.iter().find(|d| d.name == "express").unwrap();
        assert_eq!(express_dep.version, "^4.17.1");
        assert_eq!(express_dep.is_dev_dependency, false);
        
        let lodash_dep = metadata.dependencies.iter().find(|d| d.name == "lodash").unwrap();
        assert_eq!(lodash_dep.version, "^4.17.21");
        assert_eq!(lodash_dep.is_dev_dependency, false);
        
        // Check dev dependency
        let jest_dep = metadata.dependencies.iter().find(|d| d.name == "jest").unwrap();
        assert_eq!(jest_dep.version, "^27.0.0");
        assert_eq!(jest_dep.is_dev_dependency, true);
    }

    #[test]
    fn test_parse_package_json_with_multiple_licenses() {
        let json_content = r#"{
  "name": "multi-license-package",
  "version": "2.0.0",
  "licenses": [
    { "type": "MIT" },
    { "name": "Apache-2.0" }
  ],
  "dependencies": {}
}"#;

        let parser = NpmParser::new();
        let result = parser.parse(json_content);
        
        assert!(result.is_ok());
        
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "multi-license-package");
        assert_eq!(metadata.version, "2.0.0");
        // We expect at least one license to be extracted
        assert!(metadata.licenses.len() >= 1);
    }

    #[test]
    fn test_parse_minimal_package_json() {
        let json_content = r#"{
  "name": "minimal-package",
  "version": "0.1.0"
}"#;

        let parser = NpmParser::new();
        let result = parser.parse(json_content);
        
        assert!(result.is_ok());
        
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "minimal-package");
        assert_eq!(metadata.version, "0.1.0");
        assert_eq!(metadata.description, None);
        assert!(metadata.licenses.is_empty());
        assert!(metadata.dependencies.is_empty());
    }

    #[test]
    fn test_parse_invalid_json() {
        let json_content = r#"{
  "name": "invalid-package",
  "version": 
}"#;

        let parser = NpmParser::new();
        let result = parser.parse(json_content);
        
        assert!(matches!(result, Err(MetadataError::ParseError(_))));
    }
}