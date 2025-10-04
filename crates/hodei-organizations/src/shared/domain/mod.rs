pub mod account;
pub mod ou;
pub mod scp;
pub mod hrn {
    pub use policies::shared::domain::hrn::Hrn;
    // Compat helper para tests legacy que usaban Hrn::generate("ou")
    pub fn generate(resource_type: &str) -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            resource_type.to_string(),
            format!("{}-gen", resource_type),
        )
    }
}

pub use account::Account;
pub use ou::OrganizationalUnit;
pub use scp::ServiceControlPolicy;
