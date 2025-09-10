// Test the schema parsing after fix
use cedar_policy::Schema;
use std::fs;
use std::str::FromStr;

fn main() {
    let schema_content = fs::read_to_string("../security/schema/policy_schema.cedarschema").unwrap();
    println!("Schema content length: {}", schema_content.len());
    
    // Show context around position 1175
    let start = 1175_i32.saturating_sub(50) as usize;
    let end = ((1175 + 50) as usize).min(schema_content.len());
    println!("Context around position 1175:");
    println!("{}", &schema_content[start..end]);
    
    // Try to parse it as Cedar DSL
    match Schema::from_str(&schema_content) {
        Ok(_) => println!("Schema parsed successfully as Cedar DSL!"),
        Err(e) => {
            println!("Schema parsing failed as Cedar DSL: {:?}", e);
        }
    }
}