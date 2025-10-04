use crate::shared::domain::scp::ServiceControlPolicy;
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_scp_new() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    
    assert_eq!(scp.hrn, hrn);
    assert_eq!(scp.name, name);
    assert_eq!(scp.document, document);
}

#[test]
fn test_scp_clone() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    let cloned_scp = scp.clone();
    
    assert_eq!(scp.hrn, cloned_scp.hrn);
    assert_eq!(scp.name, cloned_scp.name);
    assert_eq!(scp.document, cloned_scp.document);
}

#[test]
fn test_scp_debug() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    let debug_str = format!("{:?}", scp);
    
    assert!(debug_str.contains("ServiceControlPolicy"));
    assert!(debug_str.contains("test-scp"));
    assert!(debug_str.contains("Test SCP"));
}