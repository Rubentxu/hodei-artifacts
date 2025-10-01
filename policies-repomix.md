This file is a merged representation of a subset of the codebase, containing specifically included files and files not matching ignore patterns, combined into a single document by Repomix.

# File Summary

## Purpose
This file contains a packed representation of a subset of the repository's contents that is considered the most important context.
It is designed to be easily consumable by AI systems for analysis, code review,
or other automated processes.

## File Format
The content is organized as follows:
1. This summary section
2. Repository information
3. Directory structure
4. Repository files (if enabled)
5. Multiple file entries, each consisting of:
  a. A header with the file path (## File: path/to/file)
  b. The full contents of the file in a code block

## Usage Guidelines
- This file should be treated as read-only. Any changes should be made to the
  original repository files, not this packed version.
- When processing this file, use the file path to distinguish
  between different files in the repository.
- Be aware that this file may contain sensitive information. Handle it with
  the same level of security as you would the original repository.

## Notes
- Some files may have been excluded based on .gitignore rules and Repomix's configuration
- Binary files are not included in this packed representation. Please refer to the Repository Structure section for a complete list of file paths, including binary files
- Only files matching these patterns are included: *.rs, *.toml, *.md
- Files matching these patterns are excluded: target/**, node_modules/**
- Files matching patterns in .gitignore are excluded
- Files matching default ignore patterns are excluded
- Files are sorted by Git change count (files with more changes are at the bottom)

# Directory Structure
```
Cargo.toml
test_schema.rs
```

# Files

## File: test_schema.rs
```rust
use cedar_policy::{Schema, SchemaFragment};

fn main() {
    // Test the complete schema
    let schema_str = r#"
    entity Principal { };
    
    entity User extends Principal {
        name: String,
        email: String,
        group_hrns: Set<String>,
        tags: Set<String>
    };
    
    entity Group {
        name: String,
        namespace: String,
        tags: Set<String>
    };
    
    entity ServiceAccount extends Principal {
        name: String,
        namespace: String,
        annotations: Record,
        tags: Set<String>
    };
    
    entity Namespace {
        name: String,
        annotations: Record,
        tags: Set<String>
    };
    
    entity Artifact {
        repository_id: String,
        name: String,
        version: String,
        owner: String,
        tags: Set<String>
    };
    
    entity Repository {
        name: String,
        owner: String,
        visibility: String,
        tags: Set<String>
    };
    
    entity Organization {
        name: String,
        status: String,
        primary_region: String
    };
    
    entity Policy {
        name: String,
        owner: String,
        version: String,
        status: String
    };
    
    action uploadArtifact appliesTo {
        principal: Principal,
        resource: Artifact
    };
    
    action downloadArtifact appliesTo {
        principal: Principal,
        resource: Artifact
    };
    
    action deleteArtifact appliesTo {
        principal: Principal,
        resource: Artifact
    };
    
    action viewArtifact appliesTo {
        principal: Principal,
        resource: Artifact
    };
    
    action createRepository appliesTo {
        principal: Principal,
        resource: Repository
    };
    
    action deleteRepository appliesTo {
        principal: Principal,
        resource: Repository
    };
    
    action viewRepository appliesTo {
        principal: Principal,
        resource: Repository
    };
    
    action updateRepository appliesTo {
        principal: Principal,
        resource: Repository
    };
    
    action createOrganization appliesTo {
        principal: Principal,
        resource: Organization
    };
    
    action deleteOrganization appliesTo {
        principal: Principal,
        resource: Organization
    };
    
    action viewOrganization appliesTo {
        principal: Principal,
        resource: Organization
    };
    
    action updateOrganization appliesTo {
        principal: Principal,
        resource: Organization
    };
    
    action createPolicy appliesTo {
        principal: Principal,
        resource: Policy
    };
    
    action deletePolicy appliesTo {
        principal: Principal,
        resource: Policy
    };
    
    action viewPolicy appliesTo {
        principal: Principal,
        resource: Policy
    };
    
    action updatePolicy appliesTo {
        principal: Principal,
        resource: Policy
    };
    
    action evaluatePolicy appliesTo {
        principal: Principal,
        resource: Policy
    };
    
    action administerSystem appliesTo {
        principal: Principal,
        resource: Organization
    };
    
    action manageUsers appliesTo {
        principal: Principal,
        resource: Organization
    };
    "#;

    let result = SchemaFragment::from_cedarschema_str(schema_str);
    match result {
        Ok((fragment, warnings)) => {
            println!("Schema fragment created successfully");
            for warning in warnings {
                println!("Warning: {}", warning);
            }

            // Try to build a complete schema
            let schema_result = Schema::from_schema_fragments([fragment]);
            match schema_result {
                Ok(schema) => {
                    println!("Complete schema built successfully");

                    // Try to create a validator
                    let validator = cedar_policy::Validator::new(schema);
                    println!("Validator created successfully");
                }
                Err(e) => {
                    println!("Failed to build complete schema: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("Failed to create schema fragment: {:?}", e);
        }
    }
}
```

## File: Cargo.toml
```toml
[package]
name = "policies"
version = "0.1.0"
edition = "2024"
license = "MIT"

[dependencies]
# Core dependencies
serde = { workspace = true, features = ["derive"] }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
shared = { path = "../shared" }
chrono = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }

# Cedar Policy Engine
cedar-policy = { workspace = true }

# Database - SurrealDB for policy storage
surrealdb = { workspace = true }

# Async runtime
tokio = { workspace = true, features = ["full"] }
async-trait = { workspace = true }

## testing dependencies are only declared under [dev-dependencies]

[features]
integration = []

[dev-dependencies]
mockall = { workspace = true }
testcontainers = { workspace = true }
futures = { workspace = true }
uuid = { workspace = true }
```
