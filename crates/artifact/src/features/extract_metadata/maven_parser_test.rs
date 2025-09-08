#[cfg(test)]
mod tests {
    use super::super::maven_parser::MavenParser;
    use super::super::error::MetadataError;

    #[test]
    fn test_parse_simple_pom() {
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 
         http://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>my-artifact</artifactId>
    <version>1.0.0</version>
    <description>A sample Maven project</description>
    
    <licenses>
        <license>
            <name>Apache License, Version 2.0</name>
            <url>http://www.apache.org/licenses/LICENSE-2.0.txt</url>
        </license>
    </licenses>
    
    <dependencies>
        <dependency>
            <groupId>junit</groupId>
            <artifactId>junit</artifactId>
            <version>4.12</version>
            <scope>test</scope>
        </dependency>
    </dependencies>
</project>"#;

        let parser = MavenParser::new();
        let result = parser.parse(xml_content);
        
        assert!(result.is_ok());
        
        let metadata = result.unwrap();
        assert_eq!(metadata.group_id, "com.example");
        assert_eq!(metadata.artifact_id, "my-artifact");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.description, Some("A sample Maven project".to_string()));
        assert_eq!(metadata.licenses.len(), 1);
        assert_eq!(metadata.licenses[0], "Apache License, Version 2.0");
        assert_eq!(metadata.dependencies.len(), 1);
        
        let dep = &metadata.dependencies[0];
        assert_eq!(dep.group_id, "junit");
        assert_eq!(dep.artifact_id, "junit");
        assert_eq!(dep.version, "4.12");
        assert_eq!(dep.scope, "test");
    }

    #[test]
    fn test_parse_minimal_pom() {
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 
         http://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>minimal-artifact</artifactId>
    <version>1.0.0</version>
</project>"#;

        let parser = MavenParser::new();
        let result = parser.parse(xml_content);
        
        assert!(result.is_ok());
        
        let metadata = result.unwrap();
        assert_eq!(metadata.group_id, "com.example");
        assert_eq!(metadata.artifact_id, "minimal-artifact");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.description, None);
        assert!(metadata.licenses.is_empty());
        assert!(metadata.dependencies.is_empty());
    }

    #[test]
    fn test_parse_invalid_xml() {
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project>
    <invalid>
</project>"#;

        let parser = MavenParser::new();
        let result = parser.parse(xml_content);
        
        assert!(matches!(result, Err(MetadataError::ParseError(_))));
    }
}