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
    /// Test a provider through the gateway.
    Test {
        id: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        stream: bool,
    },
    /// Show provider credential pool state.
    Pool { id: String },
    /// List credentials for a provider.
    Credentials { id: String },
    /// Enable one credential by id.
    EnableCredential { id: String },
    /// Disable one credential by id.
    DisableCredential { id: String },
    /// Reset circuit breaker state for one credential id.
    ResetCredentialCircuit { id: String },
}

pub async fn run(cmd: ProviderCmd) -> Result<()> {
    let db = Db::open(paths::db_path()?)?;
    match cmd {
        ProviderCmd::List => list(&db),
        ProviderCmd::Add => add(&db),
        ProviderCmd::Remove { id } => remove(&db, &id),
        ProviderCmd::Edit { id } => edit(&db, &id),
        ProviderCmd::Test { id, model, stream } => test_provider(&id, model, stream).await,
        ProviderCmd::Pool { id } => pool(&id).await,
        ProviderCmd::Credentials { id } => credentials(&id).await,
        ProviderCmd::EnableCredential { id } => credential_toggle(&id, true).await,
        ProviderCmd::DisableCredential { id } => credential_toggle(&id, false).await,
        ProviderCmd::ResetCredentialCircuit { id } => reset_credential_circuit(&id).await,
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
            p.id,
            p.name,
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
    let kind_str = Select::new(
        "Kind:",
        vec![
            "anthropic",
            "openai-chat",
            "openai-responses",
            "gemini-native",
        ],
    )
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
    let auth_ref_raw =
        Text::new("Auth ref (e.g. keyring:my-key or env:MY_KEY or leave blank):").prompt()?;
    let auth_ref = if auth_ref_raw.is_empty() {
        None
    } else {
        Some(auth_ref_raw)
    };
    let priority = Text::new("Priority (lower = higher):")
        .with_default("100")
        .prompt()?;
    let priority: i32 = priority.parse().unwrap_or(100);

    // optional: set a secret in keychain right now
    if let Some(ref aref) = auth_ref {
        if let Some(key_name) = aref.strip_prefix("keyring:") {
            let store = Confirm::new(&format!(
                "Store API key in keychain under '{key_name}' now?"
            ))
            .with_default(true)
            .prompt()?;
            if store {
                let key = inquire::Password::new("API key:")
                    .without_confirmation()
                    .prompt()?;
                vibe_core::secrets::keyring_set(key_name, &key)?;
                println!("Saved to keychain.");
            }
        }
    }

    let input = ProviderInput {
        name,
        group_name: None,
        kind,
        base_url,
        protocols: vec![],
        host: None,
        avatar_url: None,
        auth_ref,
        enabled: true,
        priority,
        supports_websocket: None,
        passthrough_mode: true,
        model_aliases: vec![
            ModelAlias {
                alias: "high".into(),
                upstream_model: default_high_model(kind),
            },
            ModelAlias {
                alias: "low".into(),
                upstream_model: default_low_model(kind),
            },
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
    let p = db
        .provider_get(id)?
        .ok_or_else(|| anyhow::anyhow!("provider {id} not found"))?;
    let name = Text::new("Name:").with_default(&p.name).prompt()?;
    let base_url = Text::new("Base URL:").with_default(&p.base_url).prompt()?;
    let enabled = Confirm::new("Enabled?").with_default(p.enabled).prompt()?;
    let priority = Text::new("Priority:")
        .with_default(&p.priority.to_string())
        .prompt()?;
    let priority: i32 = priority.parse().unwrap_or(p.priority);
    let updated = db.provider_update(
        id,
        ProviderInput {
            name,
            group_name: p.group_name,
            kind: p.kind,
            base_url,
            protocols: p.protocols.clone(),
            host: p.host.clone(),
            avatar_url: p.avatar_url,
            auth_ref: p.auth_ref,
            enabled,
            priority,
            supports_websocket: p.supports_websocket,
            passthrough_mode: p.passthrough_mode,
            model_aliases: p.model_aliases,
        },
    )?;
    println!("Updated provider '{}'.", updated.name);
    Ok(())
}

async fn test_provider(id: &str, model: Option<String>, stream: bool) -> Result<()> {
    let body = serde_json::json!({ "model": model, "stream": stream });
    let v = post_json(&format!("/_vp/providers/{id}/test"), body).await?;
    println!("ok:          {}", v["ok"].as_bool().unwrap_or(false));
    println!("status:      {}", v["status"]);
    println!("latency_ms:  {}", v["latency_ms"]);
    println!("log_id:      {}", v["log_id"].as_str().unwrap_or("—"));
    if let Some(preview) = v["body_preview"].as_str().filter(|s| !s.is_empty()) {
        println!("\n--- body preview ---\n{preview}");
    }
    Ok(())
}

async fn pool(id: &str) -> Result<()> {
    let v = get_json(&format!("/_vp/providers/{id}/pool")).await?;
    println!(
        "provider:       {}",
        v["provider_id"].as_str().unwrap_or(id)
    );
    println!(
        "credentials:    total={} enabled={} available={} rate_limited={} circuit_open={}",
        v["total_credentials"],
        v["enabled_credentials"],
        v["available_credentials"],
        v["rate_limited_credentials"],
        v["open_circuit_credentials"]
    );
    if let Some(items) = v["credentials"].as_array() {
        println!(
            "\n{:<36}  {:<18}  {:<7}  {:<10}  {:<8}  error",
            "ID", "Label", "Enabled", "Circuit", "RL"
        );
        println!("{}", "-".repeat(110));
        for c in items {
            println!(
                "{:<36}  {:<18}  {:<7}  {:<10}  {:<8}  {}",
                c["credential_id"].as_str().unwrap_or(""),
                c["label"].as_str().unwrap_or(""),
                c["enabled"].as_bool().unwrap_or(false),
                c["circuit_state"].as_str().unwrap_or(""),
                c["is_rate_limited"].as_bool().unwrap_or(false),
                c["last_error"].as_str().unwrap_or(""),
            );
        }
    }
    Ok(())
}

async fn credentials(id: &str) -> Result<()> {
    let v = get_json(&format!("/_vp/providers/{id}/credentials")).await?;
    let Some(items) = v.as_array() else {
        return Ok(());
    };
    println!(
        "{:<36}  {:<20}  {:<7}  {:<6}  auth",
        "ID", "Label", "Enabled", "Prio"
    );
    println!("{}", "-".repeat(96));
    for c in items {
        let auth = if c["oauth_access_token"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        {
            "oauth"
        } else {
            c["auth_ref"].as_str().unwrap_or("—")
        };
        println!(
            "{:<36}  {:<20}  {:<7}  {:<6}  {}",
            c["id"].as_str().unwrap_or(""),
            c["label"].as_str().unwrap_or(""),
            c["enabled"].as_bool().unwrap_or(false),
            c["priority"].as_i64().unwrap_or(0),
            auth,
        );
    }
    Ok(())
}

async fn credential_toggle(id: &str, enabled: bool) -> Result<()> {
    let action = if enabled { "enable" } else { "disable" };
    let v = post_empty(&format!("/_vp/credentials/{id}/{action}")).await?;
    println!(
        "{} credential {} ({})",
        if enabled { "Enabled" } else { "Disabled" },
        v["id"].as_str().unwrap_or(id),
        v["label"].as_str().unwrap_or(""),
    );
    Ok(())
}

async fn reset_credential_circuit(id: &str) -> Result<()> {
    post_empty(&format!("/_vp/credentials/{id}/circuit/reset")).await?;
    println!("Reset credential circuit {id}.");
    Ok(())
}

async fn get_json(path: &str) -> Result<serde_json::Value> {
    let url = format!("{}{path}", super::configured_base_url()?);
    let resp = reqwest::get(&url).await?;
    response_json(resp).await
}

async fn post_empty(path: &str) -> Result<serde_json::Value> {
    let url = format!("{}{path}", super::configured_base_url()?);
    let resp = reqwest::Client::new().post(url).send().await?;
    if resp.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(serde_json::json!({}));
    }
    response_json(resp).await
}

async fn post_json(path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
    let url = format!("{}{path}", super::configured_base_url()?);
    let resp = reqwest::Client::new().post(url).json(&body).send().await?;
    response_json(resp).await
}

async fn response_json(resp: reqwest::Response) -> Result<serde_json::Value> {
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("request failed ({status}): {text}");
    }
    if text.trim().is_empty() {
        Ok(serde_json::json!({}))
    } else {
        Ok(serde_json::from_str(&text)?)
    }
}

fn default_high_model(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::Anthropic => "claude-opus-4-7".into(),
        // Codex / OpenAI Chat: align with the current codex-rs default family; this is not a fixed enum and aliases can be edited in DB.
        _ => "gpt-5.3-codex".into(),
    }
}

fn default_low_model(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::Anthropic => "claude-haiku-4-5-20251001".into(),
        _ => "gpt-5.1-codex-mini".into(),
    }
}
