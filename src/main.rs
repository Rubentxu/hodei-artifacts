//! Hodei Artifacts API - Temporarily Disabled During Migration
//! 
//! The policies crate has been refactored:
//! - Old: policies::features::create_policy::*
//! - New: policies::infrastructure::validator::*
//! 
//! Policy CRUD operations have been moved to domain crates:
//! - IAM policies: hodei-iam
//! - SCPs: hodei-organizations
//!
//! To restore the API, the following migrations are needed:
//! 1. Update all imports from policies::features::* to new locations
//! 2. Migrate policy CRUD handlers to use hodei-iam
//! 3. Update AppState to remove obsolete use cases
//! 4. Reimplement playground/analysis features or remove them
//!
//! See src_disabled_migration_needed/ for the original code.

fn main() {
    eprintln!("╔══════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                                                                              ║");
    eprintln!("║  Hodei Artifacts API - MIGRATION IN PROGRESS                                ║");
    eprintln!("║                                                                              ║");
    eprintln!("║  The API binary has been temporarily disabled during the policies crate      ║");
    eprintln!("║  refactoring. The domain crates (kernel, hodei-iam, hodei-organizations,    ║");
    eprintln!("║  hodei-authorizer) are fully functional and tested.                         ║");
    eprintln!("║                                                                              ║");
    eprintln!("║  For API migration details, see:                                            ║");
    eprintln!("║  - src_disabled_migration_needed/README.md                                  ║");
    eprintln!("║  - TEST_COVERAGE_EXPANSION_SUMMARY.md                                       ║");
    eprintln!("║                                                                              ║");
    eprintln!("║  Test coverage stats:                                                       ║");
    eprintln!("║  ✅ kernel:              222 tests (91.58%% domain coverage)                ║");
    eprintln!("║  ✅ hodei-organizations: 100 tests                                          ║");
    eprintln!("║  ✅ hodei-iam:            53 tests                                          ║");
    eprintln!("║  ✅ hodei-authorizer:     11 tests                                          ║");
    eprintln!("║                                                                              ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════════════════╝");
    
    std::process::exit(1);
}
