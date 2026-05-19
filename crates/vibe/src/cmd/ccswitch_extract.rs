//! `vibe ccswitch extract` — verify CC Switch read-only extraction (no secrets printed).

use anyhow::Result;
use serde_json::json;
use vibe_core::ccswitch::{extract_default, redact_value, CcSwitchSnapshot};

pub fn run() -> Result<()> {
    let snap = extract_default()?;
    print_summary(&snap);
    Ok(())
}

fn print_summary(snap: &CcSwitchSnapshot) {
    println!("=== CC Switch extract ===\n");
    println!("root:     {}", snap.root.display());
    println!("db:       {}", snap.db_path.display());
    println!("schema:   v{}", snap.schema_version);
    println!(
        "settings: {}",
        if snap.settings.is_some() {
            snap.settings_path.display().to_string()
        } else {
            "(missing)".into()
        }
    );

    println!("\nproviders: {}", snap.providers.len());
    for (app, count) in snap.providers_by_app() {
        println!("  {app}: {count}");
    }

    println!("\neffective current (settings.json overrides DB):");
    for app in [
        "claude", "codex", "gemini", "opencode", "openclaw", "hermes",
    ] {
        if let Some(id) = snap.effective_current.get(app) {
            println!("  {app}: {id}");
        }
    }

    println!("\ndb settings keys: {}", snap.db_settings.len());
    for key in snap.db_settings.keys() {
        println!("  {key}");
    }

    if !snap.proxy_configs.is_empty() {
        println!("\nproxy_config:");
        for row in &snap.proxy_configs {
            println!(
                "  {} proxy_enabled={} enabled={} {}:{}",
                row.app_type, row.proxy_enabled, row.enabled, row.listen_address, row.listen_port
            );
        }
    }

    println!("\nproviders (redacted settings_config):");
    for p in &snap.providers {
        let redacted = redact_value(&p.settings_config);
        println!(
            "  [{}] {} is_current_db={} failover={} endpoints={}",
            p.app_type,
            p.id,
            p.is_current_in_db,
            p.in_failover_queue,
            p.custom_endpoints.len()
        );
        println!("    name: {}", p.name);
        println!("    settings_config: {}", redacted);
    }

    if let Some(settings) = &snap.settings {
        let lang = settings.language.as_deref().unwrap_or("-");
        println!(
            "\nsettings.json: language={lang} local_proxy={}",
            settings.enable_local_proxy
        );
    }

    println!("\n(full snapshot as redacted JSON available via RUST_LOG=debug or future API)");
    let _ = json!({ "ok": true, "providerCount": snap.providers.len() });
}
