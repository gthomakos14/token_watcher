//! Helpers shared by unit tests.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock, RwLockReadGuard};

/// Linux refuses to exec a file that any process has open for writing ("Text file busy").
/// If another test thread forks while a script is being written, the child briefly holds
/// that write handle. Scripts are written under the write lock, and tests that spawn
/// processes hold the read lock, so the two never overlap.
static SPAWN_LOCK: RwLock<()> = RwLock::new(());

/// Writes an executable `/bin/sh` script named `name` into `dir`, standing in for a CLI.
/// Create every script a test needs before calling [`spawn_guard`], or it will deadlock.
pub fn fake_cli(dir: &Path, name: &str, body: &str) -> PathBuf {
    let _write = SPAWN_LOCK.write().unwrap_or_else(PoisonError::into_inner);
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Hold this while running code that spawns processes.
pub fn spawn_guard() -> RwLockReadGuard<'static, ()> {
    SPAWN_LOCK.read().unwrap_or_else(PoisonError::into_inner)
}
