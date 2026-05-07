//! Resolves a `Provider.auth_ref` to a real secret.
//!
//! Supported schemes:
//! - `keyring:<name>` → OS keychain entry under service `vibe-plus`, account `<name>`
//! - `env:<VAR>` → environment variable
//! - `literal:<value>` → inline (discouraged; only for dev)

use anyhow::{Context, Result};

const SERVICE: &str = "vibe-plus";

pub fn resolve(auth_ref: &str) -> Result<String> {
    if let Some(name) = auth_ref.strip_prefix("keyring:") {
        let entry = keyring::Entry::new(SERVICE, name)?;
        Ok(entry.get_password()?)
    } else if let Some(var) = auth_ref.strip_prefix("env:") {
        std::env::var(var).with_context(|| format!("env var {var} not set"))
    } else if let Some(v) = auth_ref.strip_prefix("literal:") {
        Ok(v.to_string())
    } else {
        anyhow::bail!("unknown auth_ref scheme: {auth_ref}")
    }
}

pub fn keyring_set(name: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, name)?;
    entry.set_password(value)?;
    Ok(())
}

pub fn keyring_delete(name: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, name)?;
    entry.delete_credential()?;
    Ok(())
}
