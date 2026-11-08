//! Local Git capture/restore with OS-verified encrypted scratch storage.
mod repository;
mod runner;
mod workspace;

pub use repository::{capture, restore};
pub use runner::Options;
pub use workspace::{Encryption, Workspace};
