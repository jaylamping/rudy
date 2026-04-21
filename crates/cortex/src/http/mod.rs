//! Plaintext HTTP server and SPA static bundle.

pub mod headers;
mod server;
mod spa;

pub use server::run;
pub(crate) use server::spa_present;
