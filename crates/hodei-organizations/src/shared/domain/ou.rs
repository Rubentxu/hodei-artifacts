use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnit {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
    pub child_ous: HashSet<Hrn>,
    pub child_accounts: HashSet<Hrn>,
    pub attached_scps: HashSet<Hrn>,
}

impl OrganizationalUnit {
    pub fn new(name: String, parent_hrn: Hrn) -> Self {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            name.clone(),
        );
        Self {
            hrn,
            name,
            parent_hrn,
            child_ous: HashSet::new(),
            child_accounts: HashSet::new(),
            attached_scps: HashSet::new(),
        }
    }
    
    pub fn add_child_ou(&mut self, child_hrn: Hrn) {
        self.child_ous.insert(child_hrn);
    }
    
    pub fn remove_child_ou(&mut self, child_hrn: &Hrn) {
        self.child_ous.remove(child_hrn);
    }
    
    pub fn add_child_account(&mut self, account_hrn: Hrn) {
        self.child_accounts.insert(account_hrn);
    }
    
    pub fn remove_child_account(&mut self, account_hrn: &Hrn) {
        self.child_accounts.remove(account_hrn);
    }
    
    pub fn attach_scp(&mut self, scp_hrn: Hrn) {
        self.attached_scps.insert(scp_hrn);
    }
    
    pub fn detach_scp(&mut self, scp_hrn: &Hrn) {
        self.attached_scps.remove(scp_hrn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_ou_is_valid() {
        let parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-1".to_string(),
        );
        let ou = OrganizationalUnit::new("TestOU".to_string(), parent_hrn.clone());
        
        assert_eq!(ou.name, "TestOU");
        assert_eq!(ou.parent_hrn, parent_hrn);
        assert!(ou.child_ous.is_empty());
        assert!(ou.child_accounts.is_empty());
        assert!(ou.attached_scps.is_empty());
        assert!(!ou.hrn.to_string().is_empty());
    }
    
    #[test]
    fn test_add_child_ou() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let child_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "child-1".to_string(),
        );
        ou.add_child_ou(child_hrn.clone());
        
        assert!(ou.child_ous.contains(&child_hrn));
        assert_eq!(ou.child_ous.len(), 1);
    }
    
    #[test]
    fn test_remove_child_ou() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let child_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "child-2".to_string(),
        );
        ou.add_child_ou(child_hrn.clone());
        
        assert!(ou.child_ous.contains(&child_hrn));
        
        ou.remove_child_ou(&child_hrn);
        assert!(!ou.child_ous.contains(&child_hrn));
        assert_eq!(ou.child_ous.len(), 0);
    }
    
    #[test]
    fn test_add_child_account() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let account_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-1".to_string(),
        );
        ou.add_child_account(account_hrn.clone());
        
        assert!(ou.child_accounts.contains(&account_hrn));
        assert_eq!(ou.child_accounts.len(), 1);
    }
    
    #[test]
    fn test_remove_child_account() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let account_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-2".to_string(),
        );
        ou.add_child_account(account_hrn.clone());
        
        assert!(ou.child_accounts.contains(&account_hrn));
        
        ou.remove_child_account(&account_hrn);
        assert!(!ou.child_accounts.contains(&account_hrn));
        assert_eq!(ou.child_accounts.len(), 0);
    }
    
    #[test]
    fn test_attach_scp() {
        let mut ou = OrganizationalUnit::new(
            "TestOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-1".to_string(),
        );
        ou.attach_scp(scp_hrn.clone());
        
        assert!(ou.attached_scps.contains(&scp_hrn));
        assert_eq!(ou.attached_scps.len(), 1);
    }
    
    #[test]
    fn test_detach_scp() {
        let mut ou = OrganizationalUnit::new(
            "TestOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-2".to_string(),
        );
        ou.attach_scp(scp_hrn.clone());
        
        assert!(ou.attached_scps.contains(&scp_hrn));
        
        ou.detach_scp(&scp_hrn);
        assert!(!ou.attached_scps.contains(&scp_hrn));
        assert_eq!(ou.attached_scps.len(), 0);
    }
}
