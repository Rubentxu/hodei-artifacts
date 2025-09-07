pub mod command;
pub mod handler;
mod logic;

pub use command::{UpdateUserAttributesCommand, UpdateUserAttributesResponse};
pub use handler::handle_update_user_attributes;
