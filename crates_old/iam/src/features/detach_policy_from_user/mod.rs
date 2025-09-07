pub mod command;
pub mod handler;
pub mod logic;

pub use command::DetachPolicyFromUserCommand;
pub use handler::handle_detach_policy_from_user;