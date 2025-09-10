fn main() {
    let schema_content = std::fs::read_to_string("crates/security/schema/policy_schema.cedarschema").unwrap();
    println!("Schema content length: {}", schema_content.len());
    
    // Try to parse it
    match cedar_policy::Schema::from_str(&schema_content) {
        Ok(_) => println!("Schema parsed successfully!"),
        Err(e) => {
            println!("Schema parsing failed: {:?}", e);
            
            // Look for the character at position 1175
            if schema_content.len() > 1175 {
                let context_start = 1175.saturating_sub(50);
                let context_end = (1175 + 50).min(schema_content.len());
                println!("Context around position 1175:");
                println!("{}", &schema_content[context_start..context_end]);
                
                // Show character-by-character around the position
                println!("Characters around position 1175:");
                for i in (1175.saturating_sub(10)..=(1175 + 10)).filter(|&i| i < schema_content.len()) {
                    let ch = schema_content.chars().nth(i).unwrap();
                    if i == 1175 {
                        println!("Position {}: '{}' <<<< ERROR HERE", i, ch);
                    } else {
                        println!("Position {}: '{}'", i, ch);
                    }
                }
            }
        }
    }
}