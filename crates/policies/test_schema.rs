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
