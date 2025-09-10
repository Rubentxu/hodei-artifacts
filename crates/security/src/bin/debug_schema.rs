// Debug script to check schema parsing
use cedar_policy::Schema;
use std::fs;
use std::str::FromStr;

fn main() {
    let schema_content = fs::read_to_string("../security/schema/policy_schema.cedarschema").unwrap();
    println!("Schema content length: {}", schema_content.len());
    
    // Show the exact character at position 1175
    let pos: usize = 1175;
    if schema_content.len() > pos {
        let char_at_pos = schema_content.chars().nth(pos).unwrap();
        println!("Character at position {}: '{}'", pos, char_at_pos);
        
        // Show context around position 1175
        let start = pos.saturating_sub(50);
        let end = (pos + 50).min(schema_content.len());
        println!("Context around position {}:", pos);
        println!("{}", &schema_content[start..end]);
        
        // Show characters one by one around the position
        println!("Characters around position {}:", pos);
        for i in (pos.saturating_sub(10)..=(pos + 10)).filter(|&i| i < schema_content.len()) {
            let ch = schema_content.chars().nth(i).unwrap();
            if i == pos {
                println!("Position {}: '{}' <<<< ERROR HERE", i, ch.escape_default());
            } else {
                println!("Position {}: '{}'", i, ch.escape_default());
            }
        }
    }
    
    // Try to parse it as Cedar DSL
    match Schema::from_str(&schema_content) {
        Ok(_) => println!("Schema parsed successfully as Cedar DSL!"),
        Err(e) => {
            println!("Schema parsing failed as Cedar DSL: {:?}", e);
        }
    }
    
    // Also try to parse it as JSON (which is what might be causing the error)
    match Schema::from_json_str(&schema_content) {
        Ok(_) => println!("Schema parsed successfully as JSON!"),
        Err(e) => {
            println!("Schema parsing failed as JSON: {:?}", e);
        }
    }
}