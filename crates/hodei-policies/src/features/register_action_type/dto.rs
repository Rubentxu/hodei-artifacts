//! Data Transfer Objects for the register_action_type feature
//!
//! This feature uses direct generic method calls on the use case,
//! so no command DTOs are needed. This file exists for architectural
//! consistency with the VSA pattern.
//!
//! The registration is done via: `use_case.register::<MyAction>()`
//! instead of passing a command object.

// Placeholder: No DTOs needed for this feature
// Registration is direct and generic via the use case method
