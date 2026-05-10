//! Resolves a `Provider.auth_ref` to a real secret.
//!
//! **Runtime** resolution only — no local JSON file reads (`codex-auth` removed).
//! Import Codex `auth.json` into SQLite `credentials.oauth_*` via local import.
//!
//! Supported schemes:
//! - `keyring:<name>`       → OS keychain (service = "vibe-plus", account = <name>)
//! - `env:<VAR>`            → environment variable
//! - `literal:<value>`      → inline value (dev/test only)

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
    } else if auth_ref.starts_with("codex-auth") {
        anyhow::bail!(
            "auth_ref scheme `codex-auth` was removed: import ~/.codex/auth.json via UI Import once so tokens are stored in the gateway database (credentials.oauth_*), then use OAuth-only credentials or env:/keyring:/literal:"
        )
    } else if auth_ref.starts_with("file:") {
        anyhow::bail!(
            "auth_ref scheme `file:` was removed: store secrets in env:, keyring:, literal:, or import OAuth into the database"
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_scheme() {
        assert_eq!(resolve("literal:hello").unwrap(), "hello");
    }

    #[test]
    fn unknown_scheme_is_err() {
        assert!(resolve("ftp:something").is_err());
    }

    #[test]
    fn codex_auth_rejected() {
        assert!(resolve("codex-auth").is_err());
    }

    #[test]
    fn file_scheme_rejected() {
        assert!(resolve("file:/x.json#token").is_err());
    }
}
