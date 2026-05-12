use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum ClientCmd {
    /// Show whether a client is pointed at the local proxy.
    Status { client: String },
    /// Run client-specific takeover checks.
    Doctor { client: String },
}

pub async fn run(cmd: ClientCmd) -> Result<()> {
    match cmd {
        ClientCmd::Status { client } => show_status(&client).await,
        ClientCmd::Doctor { client } => doctor(&client).await,
    }
}

async fn show_status(client: &str) -> Result<()> {
    let v = get_json(&format!("/_vp/clients/{client}/status")).await?;
    println!(
        "client:           {}",
        v["client"].as_str().unwrap_or(client)
    );
    println!(
        "config:           {}",
        v["config_path"].as_str().unwrap_or("(unknown)")
    );
    println!(
        "exists:           {}",
        v["config_exists"].as_bool().unwrap_or(false)
    );
    println!(
        "taken over:       {}",
        v["taken_over"].as_bool().unwrap_or(false)
    );
    println!(
        "expected URL:     {}",
        v["expected_base_url"].as_str().unwrap_or("")
    );
    println!(
        "configured URL:   {}",
        v["configured_base_url"].as_str().unwrap_or("(missing)")
    );
    if !v["auth_proxy_managed"].is_null() {
        println!(
            "proxy auth:       {}",
            v["auth_proxy_managed"].as_bool().unwrap_or(false)
        );
    }
    if let Some(overrides) = v["model_overrides_present"].as_array() {
        if !overrides.is_empty() {
            let names = overrides
                .iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            println!("model overrides:  {names}");
        }
    }
    Ok(())
}

async fn doctor(client: &str) -> Result<()> {
    let v = get_json(&format!("/_vp/clients/{client}/doctor")).await?;
    println!(
        "=== vibe client doctor: {} ===\n",
        v["client"].as_str().unwrap_or(client)
    );
    if let Some(checks) = v["checks"].as_array() {
        for c in checks {
            let ok = c["ok"].as_bool().unwrap_or(false);
            println!(
                "[{}] {:<28} {}",
                if ok { "ok" } else { "!!" },
                c["name"].as_str().unwrap_or("check"),
                c["detail"].as_str().unwrap_or("")
            );
        }
    }
    println!(
        "\nresult: {}",
        if v["ok"].as_bool().unwrap_or(false) {
            "ok"
        } else {
            "needs attention"
        }
    );
    Ok(())
}

async fn get_json(path: &str) -> Result<serde_json::Value> {
    let url = format!("{}{path}", super::configured_base_url()?);
    let resp = reqwest::get(&url).await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("request failed ({status}): {text}");
    }
    Ok(serde_json::from_str(&text)?)
}
