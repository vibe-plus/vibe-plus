use anyhow::Result;
use clap::Subcommand;
use vibe_protocol::{RouteInput, RouteTier};

#[derive(Subcommand)]
pub enum RouteCmd {
    /// List routing rules.
    List,
    /// Add a routing rule.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        match_model: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        target_model: Option<String>,
        #[arg(long, default_value = "default")]
        tier: String,
        #[arg(long, default_value_t = 100)]
        priority: i32,
    },
    /// Edit a routing rule.
    Edit {
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        match_model: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        target_model: Option<String>,
        #[arg(long, default_value = "default")]
        tier: String,
        #[arg(long, default_value_t = 100)]
        priority: i32,
    },
    /// Remove a routing rule.
    Remove { id: String },
    /// Explain which providers a model would route to.
    Explain {
        #[arg(long)]
        model: String,
        #[arg(long, default_value = "openai-responses")]
        wire: String,
    },
}

pub async fn run(cmd: RouteCmd) -> Result<()> {
    match cmd {
        RouteCmd::List => list().await,
        RouteCmd::Add {
            name,
            match_model,
            provider,
            target_model,
            tier,
            priority,
        } => {
            let input = input(name, match_model, provider, target_model, tier, priority)?;
            let v = post_json("/_vp/routes", serde_json::to_value(input)?).await?;
            print_route(&v);
            Ok(())
        }
        RouteCmd::Edit {
            id,
            name,
            match_model,
            provider,
            target_model,
            tier,
            priority,
        } => {
            let input = input(name, match_model, provider, target_model, tier, priority)?;
            let v = put_json(&format!("/_vp/routes/{id}"), serde_json::to_value(input)?).await?;
            print_route(&v);
            Ok(())
        }
        RouteCmd::Remove { id } => {
            delete(&format!("/_vp/routes/{id}")).await?;
            println!("Removed route {id}.");
            Ok(())
        }
        RouteCmd::Explain { model, wire } => explain(&model, &wire).await,
    }
}

async fn list() -> Result<()> {
    let v = get_json("/_vp/routes").await?;
    let Some(items) = v.as_array() else {
        return Ok(());
    };
    if items.is_empty() {
        println!("No routes configured.");
        return Ok(());
    }
    println!(
        "{:<36}  {:<18}  {:<24}  {:<36}  {:<18}  {:<8}  prio",
        "ID", "Name", "Match", "Provider", "Target", "Tier"
    );
    println!("{}", "-".repeat(160));
    for r in items {
        println!(
            "{:<36}  {:<18}  {:<24}  {:<36}  {:<18}  {:<8}  {}",
            r["id"].as_str().unwrap_or(""),
            r["name"].as_str().unwrap_or(""),
            r["match_model"].as_str().unwrap_or(""),
            r["target_provider_id"].as_str().unwrap_or("—"),
            r["target_model"].as_str().unwrap_or("—"),
            r["tier"].as_str().unwrap_or(""),
            r["priority"].as_i64().unwrap_or(0),
        );
    }
    Ok(())
}

fn input(
    name: String,
    match_model: String,
    provider: Option<String>,
    target_model: Option<String>,
    tier: String,
    priority: i32,
) -> Result<RouteInput> {
    Ok(RouteInput {
        name,
        match_model,
        target_provider_id: provider.filter(|s| !s.trim().is_empty()),
        target_model: target_model.filter(|s| !s.trim().is_empty()),
        tier: parse_tier(&tier)?,
        priority,
    })
}

fn parse_tier(s: &str) -> Result<RouteTier> {
    Ok(match s {
        "high" => RouteTier::High,
        "low" => RouteTier::Low,
        "default" => RouteTier::Default,
        other => anyhow::bail!("unknown tier {other}; expected high, low, or default"),
    })
}

fn print_route(v: &serde_json::Value) {
    println!("id:        {}", v["id"].as_str().unwrap_or(""));
    println!("name:      {}", v["name"].as_str().unwrap_or(""));
    println!("match:     {}", v["match_model"].as_str().unwrap_or(""));
    println!(
        "provider:  {}",
        v["target_provider_id"].as_str().unwrap_or("—")
    );
    println!("target:    {}", v["target_model"].as_str().unwrap_or("—"));
    println!("tier:      {}", v["tier"].as_str().unwrap_or(""));
    println!("priority:  {}", v["priority"]);
}

async fn explain(model: &str, wire: &str) -> Result<()> {
    let v = get_json(&format!(
        "/_vp/routes/explain?model={}&wire={}",
        url_component(model),
        url_component(wire)
    ))
    .await?;
    println!("model:  {}", v["requested_model"].as_str().unwrap_or(model));
    println!("wire:   {}", v["wire"].as_str().unwrap_or(wire));
    if v["matched_route"].is_null() {
        println!("route:  —");
    } else {
        let r = &v["matched_route"];
        println!(
            "route:  {} ({})",
            r["name"].as_str().unwrap_or(""),
            r["id"].as_str().unwrap_or("")
        );
        println!(
            "target: provider={} model={}",
            r["target_provider_id"].as_str().unwrap_or("—"),
            r["target_model"].as_str().unwrap_or("—")
        );
    }
    println!(
        "\n{:<36}  {:<20}  {:<16}  {:<24}  prio",
        "Provider", "Name", "Kind", "Upstream"
    );
    println!("{}", "-".repeat(112));
    if let Some(items) = v["candidates"].as_array() {
        for p in items {
            println!(
                "{:<36}  {:<20}  {:<16}  {:<24}  {}",
                p["provider_id"].as_str().unwrap_or(""),
                p["provider_name"].as_str().unwrap_or(""),
                p["provider_kind"].as_str().unwrap_or(""),
                p["upstream_model"].as_str().unwrap_or(""),
                p["priority"].as_i64().unwrap_or(0),
            );
        }
    }
    Ok(())
}

fn url_component(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('&', "%26")
        .replace('?', "%3F")
        .replace('#', "%23")
        .replace('+', "%2B")
}

async fn get_json(path: &str) -> Result<serde_json::Value> {
    let url = format!("{}{path}", super::configured_base_url()?);
    let resp = reqwest::get(&url).await?;
    response_json(resp).await
}

async fn post_json(path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
    let url = format!("{}{path}", super::configured_base_url()?);
    let resp = reqwest::Client::new().post(url).json(&body).send().await?;
    response_json(resp).await
}

async fn put_json(path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
    let url = format!("{}{path}", super::configured_base_url()?);
    let resp = reqwest::Client::new().put(url).json(&body).send().await?;
    response_json(resp).await
}

async fn delete(path: &str) -> Result<()> {
    let url = format!("{}{path}", super::configured_base_url()?);
    let resp = reqwest::Client::new().delete(url).send().await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("request failed ({status}): {text}");
    }
    Ok(())
}

async fn response_json(resp: reqwest::Response) -> Result<serde_json::Value> {
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("request failed ({status}): {text}");
    }
    Ok(serde_json::from_str(&text)?)
}
