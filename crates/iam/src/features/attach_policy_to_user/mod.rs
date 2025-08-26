pub mod command;
pub mod handler;
pub mod logic;

pub use command::AttachPolicyToUserCommand;
pub use handler::handle_attach_policy_to_user;