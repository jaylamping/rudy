//! `cortex` - Rudy operator-console daemon.
//!
//! See docs/decisions/0004-operator-console.md for architecture + safety.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    cortex::app::bootstrap::run(std::env::args().collect()).await
}
