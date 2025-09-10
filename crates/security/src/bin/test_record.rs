// Test the Record type parsing issue
use cedar_policy::Schema;
use std::str::FromStr;

fn main() {
    // Extract just the problematic part
    let minimal_schema = r#"
        namespace Hodei {
            entity Organization = {
                "hrn": String,
                "name": String,
                "domain": String,
                "active": Bool,
                "settings": Record<String, String>,
            };
        }
    "#;
    
    println!("Testing minimal schema...");
    match Schema::from_str(minimal_schema) {
        Ok(_) => println!("Minimal schema parsed successfully!"),
        Err(e) => {
            println!("Minimal schema parsing failed: {:?}", e);
        }
    }
    
    // Test just the Record type
    let record_test = r#"
        namespace Test {
            entity TestEntity = {
                "test_field": Record<String, String>,
            };
        }
    "#;
    
    println!("Testing Record type...");
    match Schema::from_str(record_test) {
        Ok(_) => println!("Record type test parsed successfully!"),
        Err(e) => {
            println!("Record type test failed: {:?}", e);
        }
    }
}