//! Platform-independent desktop service. No renderer access to vault keys,
//! arbitrary shell execution, or implicit filesystem exports.
use repo_stealer_git::{capture, restore, Options, Workspace};
use repo_stealer_vault::{SecretString, Vault};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    CreateVault {
        path: String,
        passphrase: String,
    },
    UnlockVault {
        path: String,
        passphrase: String,
    },
    CheckWorkspace {
        path: String,
        create: bool,
    },
    CaptureLocal {
        name: String,
        source: String,
    },
    RestoreGit {
        name: String,
        destination: String,
    },
    CreateBackup {
        destination: String,
    },
    VerifyBackup {
        path: String,
        passphrase: String,
    },
    RestoreBackup {
        path: String,
        destination: String,
        passphrase: String,
    },
}

impl Action {
    fn label(&self) -> &'static str {
        match self {
            Self::CreateVault { .. } => "Create encrypted vault",
            Self::UnlockVault { .. } => "Unlock vault",
            Self::CheckWorkspace { .. } => "Verify encrypted workspace",
            Self::CaptureLocal { .. } => "Capture repository",
            Self::RestoreGit { .. } => "Restore Git repository",
            Self::CreateBackup { .. } => "Create encrypted backup",
            Self::VerifyBackup { .. } => "Verify encrypted backup",
            Self::RestoreBackup { .. } => "Restore encrypted backup",
        }
    }
    fn cancellable(&self) -> bool {
        matches!(self, Self::CaptureLocal { .. } | Self::RestoreGit { .. })
    }
    fn needs_vault(&self) -> bool {
        matches!(
            self,
            Self::CaptureLocal { .. } | Self::RestoreGit { .. } | Self::CreateBackup { .. }
        )
    }
    fn needs_locked(&self) -> bool {
        matches!(
            self,
            Self::CreateVault { .. } | Self::UnlockVault { .. } | Self::RestoreBackup { .. }
        )
    }
    fn needs_workspace(&self) -> bool {
        matches!(self, Self::CaptureLocal { .. } | Self::RestoreGit { .. })
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub path: String,
    pub item_count: usize,
    pub total_bytes: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPage {
    pub items: Vec<Item>,
    pub total: usize,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatus {
    pub path: String,
    pub encryption: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    pub label: String,
    pub state: String,
    pub phase: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
    pub cancellable: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub version: String,
    pub platform: String,
    pub vault: Option<VaultStatus>,
    pub workspace: Option<WorkspaceStatus>,
    pub busy: bool,
    pub jobs: Vec<Job>,
    pub remote_capture: bool,
}

struct Inner {
    vault: Option<Vault>,
    summary: Option<VaultStatus>,
    workspace: Option<WorkspaceStatus>,
    jobs: Vec<Job>,
    next_id: u64,
    busy: bool,
    cancelled: Option<Arc<AtomicBool>>,
}

pub struct Engine {
    inner: Mutex<Inner>,
}
impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
impl Engine {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                vault: None,
                summary: None,
                workspace: None,
                jobs: vec![],
                next_id: 1,
                busy: false,
                cancelled: None,
            }),
        }
    }
    pub fn status(&self) -> Result<Status, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "application state unavailable")?;
        Ok(Status {
            version: env!("CARGO_PKG_VERSION").into(),
            platform: std::env::consts::OS.into(),
            vault: inner.summary.clone(),
            workspace: inner.workspace.clone(),
            busy: inner.busy,
            jobs: inner.jobs.clone(),
            remote_capture: false,
        })
    }
    pub fn lock(&self) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "application state unavailable")?;
        if inner.busy {
            return Err("Finish or cancel the active job before locking the vault.".into());
        }
        inner.vault = None;
        inner.summary = None;
        Ok(())
    }
    pub fn items(&self, query: &str, offset: usize, limit: usize) -> Result<ItemPage, String> {
        if query.len() > 256 || !(1..=100).contains(&limit) {
            return Err("Invalid item query.".into());
        }
        let inner = self
            .inner
            .lock()
            .map_err(|_| "application state unavailable")?;
        let vault = inner
            .vault
            .as_ref()
            .ok_or("Unlock a vault and wait for the active job to finish.")?;
        let query = query.to_lowercase();
        let matches: Vec<_> = vault
            .entries()
            .iter()
            .filter(|entry| entry.name.to_lowercase().contains(&query))
            .collect();
        Ok(ItemPage {
            total: matches.len(),
            items: matches
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|entry| Item {
                    name: entry.name.clone(),
                    size: entry.size,
                    sha256: entry.sha256.clone(),
                })
                .collect(),
        })
    }
    pub fn cancel(&self) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "application state unavailable")?;
        let flag = inner
            .cancelled
            .as_ref()
            .ok_or("No cancellable Git job is running.")?;
        flag.store(true, Ordering::Relaxed);
        if let Some(job) = inner.jobs.first_mut() {
            job.phase = "Cancelling safely…".into();
        }
        Ok(())
    }
    pub fn start(self: &Arc<Self>, action: Action) -> Result<u64, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "application state unavailable")?;
        if inner.busy {
            return Err("Another operation is already running.".into());
        }
        if action.needs_vault() && inner.vault.is_none() {
            return Err("Unlock a vault first.".into());
        }
        if action.needs_locked() && inner.vault.is_some() {
            return Err("Lock the current vault before opening or restoring another.".into());
        }
        if action.needs_workspace() && inner.workspace.is_none() {
            return Err("Verify encrypted Git working storage first.".into());
        }
        let id = inner.next_id;
        inner.next_id += 1;
        let flag = Arc::new(AtomicBool::new(false));
        inner.jobs.insert(
            0,
            Job {
                id,
                label: action.label().into(),
                state: "running".into(),
                phase: "Preparing operation".into(),
                started_at: now(),
                finished_at: None,
                error: None,
                cancellable: action.cancellable(),
            },
        );
        inner.jobs.truncate(20);
        inner.busy = true;
        inner.cancelled = if action.cancellable() {
            Some(flag.clone())
        } else {
            None
        };
        let vault = inner.vault.take();
        let path = inner.summary.as_ref().map(|s| s.path.clone());
        let workspace = inner.workspace.clone();
        drop(inner);
        let engine = self.clone();
        std::thread::spawn(move || {
            let mut vault = vault;
            let mut path = path;
            let mut workspace = workspace;
            let progress_engine = engine.clone();
            let options = Options {
                cancelled: flag.clone(),
                progress: Some(Arc::new(move |phase| progress_engine.phase(id, phase))),
                ..Options::default()
            };
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                perform(action, &mut vault, &mut path, &mut workspace, &options)
            })) {
                Ok(result) => result,
                Err(_) => {
                    vault = None;
                    path = None;
                    Err(
                        "The operation stopped unexpectedly. Reopen your vault before retrying."
                            .into(),
                    )
                }
            };
            if let Ok(mut inner) = engine.inner.lock() {
                inner.summary = vault
                    .as_ref()
                    .map(|v| summarize(v, path.as_deref().unwrap_or_default()));
                inner.vault = vault;
                inner.workspace = workspace;
                inner.busy = false;
                inner.cancelled = None;
                if let Some(job) = inner.jobs.iter_mut().find(|job| job.id == id) {
                    job.finished_at = Some(now());
                    match result {
                        Ok(()) => {
                            job.state = "completed".into();
                            job.phase = "Completed".into();
                        }
                        Err(error) => {
                            job.state = if flag.load(Ordering::Relaxed) {
                                "cancelled"
                            } else {
                                "failed"
                            }
                            .into();
                            job.phase = if job.state == "cancelled" {
                                "Cancelled"
                            } else {
                                "Operation failed"
                            }
                            .into();
                            job.error = Some(error.chars().take(2000).collect());
                        }
                    }
                }
            }
        });
        Ok(id)
    }
    fn phase(&self, id: u64, phase: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(job) = inner.jobs.iter_mut().find(|j| j.id == id) {
                job.phase = phase.into();
            }
        }
    }
}

fn perform(
    action: Action,
    vault: &mut Option<Vault>,
    path: &mut Option<String>,
    workspace: &mut Option<WorkspaceStatus>,
    options: &Options,
) -> Result<(), String> {
    let result = || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match action {
            Action::CreateVault {
                path: value,
                passphrase,
            } => {
                *vault = Some(Vault::create(&checked_path(&value)?, secret(passphrase)?)?);
                *path = Some(value);
            }
            Action::UnlockVault {
                path: value,
                passphrase,
            } => {
                *vault = Some(Vault::open(&checked_path(&value)?, secret(passphrase)?)?);
                *path = Some(value);
            }
            Action::CheckWorkspace { path, create } => {
                let verified = if create {
                    Workspace::init(&checked_path(&path)?)?
                } else {
                    Workspace::open(&checked_path(&path)?)?
                };
                *workspace = Some(WorkspaceStatus {
                    path: verified.root().to_string_lossy().into(),
                    encryption: format!("{:?}", verified.encryption()),
                });
            }
            Action::CaptureLocal { name, source } => {
                let root = Workspace::open(Path::new(
                    &workspace.as_ref().ok_or("workspace required")?.path,
                ))?;
                capture(
                    vault.as_mut().ok_or("vault required")?,
                    &name,
                    &checked_path(&source)?,
                    &root,
                    options,
                )?;
            }
            Action::RestoreGit { name, destination } => {
                let root = Workspace::open(Path::new(
                    &workspace.as_ref().ok_or("workspace required")?.path,
                ))?;
                restore(
                    vault.as_ref().ok_or("vault required")?,
                    &name,
                    &checked_path(&destination)?,
                    &root,
                    options,
                )?;
            }
            Action::CreateBackup { destination } => {
                vault
                    .as_ref()
                    .ok_or("vault required")?
                    .backup(&checked_path(&destination)?)?;
            }
            Action::VerifyBackup { path, passphrase } => {
                Vault::verify_backup(&checked_path(&path)?, secret(passphrase)?)?;
            }
            Action::RestoreBackup {
                path: input,
                destination,
                passphrase,
            } => {
                let password = secret(passphrase)?;
                Vault::restore(
                    &checked_path(&input)?,
                    &checked_path(&destination)?,
                    password.clone(),
                )?;
                *vault = Some(Vault::open(&checked_path(&destination)?, password)?);
                *path = Some(destination);
            }
        }
        Ok(())
    };
    result().map_err(|e| e.to_string())
}

fn secret(value: String) -> Result<SecretString, String> {
    let mut value = zeroize::Zeroizing::new(value);
    if value.is_empty() || value.len() > 4096 {
        return Err("Enter a passphrase of at most 4096 bytes.".into());
    }
    // Do not derive Debug/Serialize for actions or retain them in job history.
    Ok(SecretString::from(std::mem::take(&mut *value)))
}
fn checked_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.len() > 32768 || value.chars().any(char::is_control) {
        return Err("Choose a valid local path.".into());
    }
    Ok(PathBuf::from(value))
}
fn summarize(vault: &Vault, path: &str) -> VaultStatus {
    VaultStatus {
        path: path.into(),
        total_bytes: vault.entries().iter().map(|e| e.size).sum(),
        item_count: vault.entries().len(),
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    fn finish(engine: &Engine) -> Status {
        for _ in 0..1000 {
            let status = engine.status().unwrap();
            if !status.busy {
                return status;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("background job did not finish");
    }
    #[test]
    fn locked_operations_are_gated_and_passwords_are_not_status_data() {
        let engine = Arc::new(Engine::new());
        assert!(engine
            .start(Action::CreateBackup {
                destination: "out".into()
            })
            .is_err());
        assert!(engine
            .start(Action::CaptureLocal {
                name: "test".into(),
                source: "source".into()
            })
            .is_err());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vault").to_str().unwrap().to_owned();
        engine
            .start(Action::CreateVault {
                path: path.clone(),
                passphrase: "synthetic desktop passphrase".into(),
            })
            .unwrap();
        let status = finish(&engine);
        assert_eq!(status.jobs[0].state, "completed");
        assert!(status.vault.is_some());
        assert!(engine
            .start(Action::UnlockVault {
                path: path.clone(),
                passphrase: "ignored".into()
            })
            .is_err());
        engine.lock().unwrap();
        assert!(engine.status().unwrap().vault.is_none());
        engine
            .start(Action::UnlockVault {
                path,
                passphrase: "wrong password".into(),
            })
            .unwrap();
        let status = finish(&engine);
        assert_eq!(status.jobs[0].state, "failed");
        assert!(status.vault.is_none());
        assert!(!status.jobs[0]
            .error
            .as_ref()
            .unwrap()
            .contains("wrong password"));
        assert!(engine.cancel().is_err());
    }
}
