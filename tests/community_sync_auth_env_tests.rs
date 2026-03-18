use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().expect("env lock")
}

fn restore_env(sync_token: Option<String>, admin_token: Option<String>) {
    match sync_token {
        Some(value) => {
            // SAFETY: serialized by ENV_LOCK in this test module.
            unsafe { std::env::set_var("SYNC_TOKEN", value) }
        }
        None => {
            // SAFETY: serialized by ENV_LOCK in this test module.
            unsafe { std::env::remove_var("SYNC_TOKEN") }
        }
    }

    match admin_token {
        Some(value) => {
            // SAFETY: serialized by ENV_LOCK in this test module.
            unsafe { std::env::set_var("ADMIN_TOKEN", value) }
        }
        None => {
            // SAFETY: serialized by ENV_LOCK in this test module.
            unsafe { std::env::remove_var("ADMIN_TOKEN") }
        }
    }
}

#[test]
fn load_sync_auth_from_env_filters_empty_values() {
    let _guard = lock_env();
    let old_sync = std::env::var("SYNC_TOKEN").ok();
    let old_admin = std::env::var("ADMIN_TOKEN").ok();

    // SAFETY: serialized by ENV_LOCK in this test module.
    unsafe {
        std::env::set_var("SYNC_TOKEN", "");
        std::env::set_var("ADMIN_TOKEN", "admin-secret");
    }

    let (sync_token, admin_token) = stophammer::community::load_sync_auth_from_env();
    assert_eq!(sync_token, None);
    assert_eq!(admin_token.as_deref(), Some("admin-secret"));

    restore_env(old_sync, old_admin);
}

#[test]
fn load_sync_auth_from_env_prefers_explicit_sync_token() {
    let _guard = lock_env();
    let old_sync = std::env::var("SYNC_TOKEN").ok();
    let old_admin = std::env::var("ADMIN_TOKEN").ok();

    // SAFETY: serialized by ENV_LOCK in this test module.
    unsafe {
        std::env::set_var("SYNC_TOKEN", "sync-secret");
        std::env::set_var("ADMIN_TOKEN", "admin-secret");
    }

    let (sync_token, admin_token) = stophammer::community::load_sync_auth_from_env();
    assert_eq!(sync_token.as_deref(), Some("sync-secret"));
    assert_eq!(admin_token.as_deref(), Some("admin-secret"));

    restore_env(old_sync, old_admin);
}
