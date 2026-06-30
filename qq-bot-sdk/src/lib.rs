pub mod auth;
pub mod client;
pub mod error;
pub mod gateway;
pub mod media;
pub mod message;
pub mod model;
pub mod throttle;

pub use client::QQBotClient;
pub use error::{Error, Result};
pub use model::*;

pub mod prelude {
    pub use crate::auth::*;
    pub use crate::client::*;
    pub use crate::error::*;
    pub use crate::gateway::*;
    pub use crate::media::*;
    pub use crate::message::*;
    pub use crate::model::*;
    pub use crate::throttle::*;
}
