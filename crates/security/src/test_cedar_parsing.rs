#[cfg(test)]
mod tests {
    use cedar_policy::Schema;
    use std::path::PathBuf;
    use std::str::FromStr;
    
    #[test]
    fn test_simple_cedar_parsing() {
        let schema_content = r#"
            namespace Test {
                entity User = {
                    "name": String,
                };
                
                action ReadUser appliesTo {
                    principal: [User],
                    resource: [User]
                };
            }
        "#;
        
        let result = Schema::from_str(schema_content);
        println!("Parsing result: {:?}", result);
        assert!(result.is_ok(), "Failed to parse simple schema: {:?}", result.err());
    }
    
    #[test]
    fn test_actual_schema_parsing() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("schema/policy_schema.cedarschema");
        
        let schema_content = std::fs::read_to_string(&path).unwrap();
        println!("Schema file path: {:?}", path);
        println!("Schema content length: {}", schema_content.len());
        println!("Schema content preview: {}", &schema_content[..std::cmp::min(200, schema_content.len())]);
        
        let result = Schema::from_str(&schema_content);
        println!("Parsing result: {:?}", result);
        assert!(result.is_ok(), "Failed to parse actual schema: {:?}", result.err());
    }
}