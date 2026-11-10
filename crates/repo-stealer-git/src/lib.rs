//! Local Git capture/restore with OS-verified encrypted scratch storage.
mod remote;
mod repository;
mod runner;
mod workspace;

pub use remote::{capture_remote, CaptureProvenance, Credentials, ExternalInventory, RemoteUrl};
pub use repository::{capture, restore};
pub use runner::Options;
pub use workspace::{Encryption, Workspace};
