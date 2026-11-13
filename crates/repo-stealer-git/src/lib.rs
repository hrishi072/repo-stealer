//! Git capture/restore and guarded new-repository transfer in OS-verified encrypted storage.
mod remote;
mod repository;
mod runner;
mod workspace;

pub use remote::{
    capture_remote, observe_remote, Advertisement, CaptureProvenance, Credentials,
    ExternalInventory, RemoteUrl,
};
pub use repository::{capture, restore, snapshot_refs, SnapshotRefs};
pub use runner::Options;
pub use workspace::{Encryption, Workspace};

mod transfer;
pub use transfer::VerifiedTransfer;

mod lfs;
pub use lfs::LfsObject;

mod lfs_pool;

mod lfs_http;
