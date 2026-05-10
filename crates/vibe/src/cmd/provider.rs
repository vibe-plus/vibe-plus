use anyhow::Result;
use clap::Subcommand;
use inquire::{Confirm, Select, Text};
use vibe_core::paths;
use vibe_db::Db;
use vibe_protocol::{ModelAlias, ProviderInput, ProviderKind};

#[derive(Subcommand)]
pub enum ProviderCmd {
    /// List all configured providers.
    List,
    /// Add a new provider interactively.
    Add,
    /// Remove a provider by id.
    Remove { id: String },
    /// Edit a provider interactively.
    Edit { id: String },
}

pub async fn run(cmd: ProviderCmd) -> Result<()> {
    let db = Db::open(paths::db_path()?)?;
    match cmd {
        ProviderCmd::List => list(&db),
        ProviderCmd::Add => add(&db),
        ProviderCmd::Remove { id } => remove(&db, &id),
        ProviderCmd::Edit { id } => edit(&db, &id),
    }
}

fn list(db: &Db) -> Result<()> {
    let providers = db.provider_list()?;
    if providers.is_empty() {
        println!("No providers configured. Run `vibe provider add` to add one.");
        return Ok(());
    }
    println!(
        "{:<36}  {:<20}  {:<14}  {:>5}  {:<7}  {:>5}  {:>5}  {:>5}",
        "ID", "Name", "Kind", "Prio", "Enabled", "Creds", "Avail", "RL"
    );
    println!("{}", "-".repeat(118));
    for p in &providers {
        let creds = db.credential_list_for_provider(&p.id).unwrap_or_default();
        let total_creds = creds.len() as i64;
        let enabled_creds = creds.iter().filter(|c| c.enabled).count() as i64;
        let now = chrono::Utc::now().timestamp();
        let rate_limited = creds
            .iter()
            .filter(|c| {
                let req_exhausted = c.rl_requests_remaining == Some(0)
                    && c.rl_requests_reset_at.map(|t| t > now).unwrap_or(false);
                let tok_exhausted = c.rl_tokens_remaining == Some(0)
                    && c.rl_tokens_reset_at.map(|t| t > now).unwrap_or(false);
                req_exhausted || tok_exhausted
            })
            .count() as i64;
        let available = (enabled_creds - rate_limited).max(0);
        println!(
            "{:<36}  {:<20}  {:<14}  {:>5}  {:<7}  {:>5}  {:>5}  {:>5}",
            p.id, p.name,
            format!("{:?}", p.kind),
            p.priority,
            if p.enabled { "yes" } else { "no" },
            total_creds,
            available,
            rate_limited,
        );
        if let Some(err) = creds.iter().find_map(|c| c.last_error.as_deref()) {
            println!("  ↳ last credential error: {err}");
        }
    }
    Ok(())
}

fn add(db: &Db) -> Result<()> {
    let name = Text::new("Provider name:").prompt()?;
    let kind_str =
        Select::new("Kind:", vec!["anthropic", "openai-chat", "openai-responses", "gemini-native"])
            .prompt()?;
    let kind = match kind_str {
        "anthropic" => ProviderKind::Anthropic,
        "openai-responses" => ProviderKind::OpenaiResponses,
        "gemini-native" => ProviderKind::GeminiNative,
        _ => ProviderKind::OpenaiChat,
    };
    let default_url = match kind {
        ProviderKind::Anthropic => "https://api.anthropic.com",
        _ => "https://api.openai.com",
    };
    let base_url = Text::new("Base URL:").with_default(default_url).prompt()?;
    let auth_ref_raw = Text::new("Auth ref (e.g. keyring:my-key or env:MY_KEY or leave blank):").prompt()?;
    let auth_ref = if auth_ref_raw.is_empty() { None } else { Some(auth_ref_raw) };
    let priority = Text::new("Priority (lower = higher):").with_default("100").prompt()?;
    let priority: i32 = priority.parse().unwrap_or(100);

    // optional: set a secret in keychain right now
    if let Some(ref aref) = auth_ref {
        if let Some(key_name) = aref.strip_prefix("keyring:") {
            let store = Confirm::new(&format!("Store API key in keychain under '{key_name}' now?")).with_default(true).prompt()?;
            if store {
                let key = inquire::Password::new("API key:").without_confirmation().prompt()?;
                vibe_core::secrets::keyring_set(key_name, &key)?;
                println!("Saved to keychain.");
            }
        }
    }

    let input = ProviderInput {
        name,
        kind,
        base_url,
        auth_ref,
        enabled: true,
        priority,
        model_aliases: vec![
            ModelAlias { alias: "high".into(), upstream_model: default_high_model(kind) },
            ModelAlias { alias: "low".into(),  upstream_model: default_low_model(kind) },
        ],
    };
    let p = db.provider_insert(input)?;
    println!("Provider '{}' created with id {}.", p.name, p.id);
    Ok(())
}

fn remove(db: &Db, id: &str) -> Result<()> {
    db.provider_delete(id)?;
    println!("Removed provider {id}.");
    Ok(())
}

fn edit(db: &Db, id: &str) -> Result<()> {
    let p = db.provider_get(id)?.ok_or_else(|| anyhow::anyhow!("provider {id} not found"))?;
    let name = Text::new("Name:").with_default(&p.name).prompt()?;
    let base_url = Text::new("Base URL:").with_default(&p.base_url).prompt()?;
    let enabled = Confirm::new("Enabled?").with_default(p.enabled).prompt()?;
    let priority = Text::new("Priority:").with_default(&p.priority.to_string()).prompt()?;
    let priority: i32 = priority.parse().unwrap_or(p.priority);
    let updated = db.provider_update(id, ProviderInput {
        name,
        kind: p.kind,
        base_url,
        auth_ref: p.auth_ref,
        enabled,
        priority,
        model_aliases: p.model_aliases,
    })?;
    println!("Updated provider '{}'.", updated.name);
    Ok(())
}

fn default_high_model(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::Anthropic => "claude-opus-4-7".into(),
        // Codex / OpenAI Chat：与 codex-rs 当前默认家族对齐（非固定枚举，可在 DB 里改别名）
        _ => "gpt-5.3-codex".into(),
    }
}

fn default_low_model(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::Anthropic => "claude-haiku-4-5-20251001".into(),
        _ => "gpt-5.1-codex-mini".into(),
    }
}
