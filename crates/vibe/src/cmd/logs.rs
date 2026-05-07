use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct LogsArgs {
    /// Follow new entries as they arrive.
    #[arg(long, short)]
    pub tail: bool,
    /// How many recent entries to show.
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
}

pub async fn run(args: LogsArgs) -> Result<()> {
    let base = format!("http://127.0.0.1:{}", super::DEFAULT_PORT);

    if args.tail {
        println!("Connecting to live log stream (Ctrl-C to stop)…");
        use tokio_tungstenite::connect_async;
        use futures_util::StreamExt;
        let ws_url = base.replace("http://", "ws://") + "/_vp/ws";
        let (mut ws, _) = connect_async(&ws_url).await?;
        while let Some(msg) = ws.next().await {
            let msg = msg?;
            if let tokio_tungstenite::tungstenite::Message::Text(txt) = msg {
                let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or_default();
                if v["type"] == "log-appended" {
                    let log = &v;
                    println!(
                        "{} [{:>3}ms] {:?} → {:?}",
                        log["started_at"].as_i64().unwrap_or(0),
                        log["latency_ms"].as_i64().unwrap_or(0),
                        log["requested_model"],
                        log["upstream_model"],
                    );
                }
            }
        }
    } else {
        let url = format!("{base}/_vp/logs?limit={}", args.limit);
        let resp: serde_json::Value = reqwest::get(&url).await?.json().await?;
        if let Some(items) = resp["items"].as_array() {
            println!("{:<24}  {:>6}  {:>8}ms  {:>6}in  {:>6}out  model", "id", "status", "latency", "tokens", "tokens");
            println!("{}", "-".repeat(80));
            for item in items {
                println!(
                    "{:<24}  {:>6}  {:>8}ms  {:>6}  {:>6}  {}",
                    item["id"].as_str().unwrap_or(""),
                    item["status_code"].as_i64().unwrap_or(0),
                    item["latency_ms"].as_i64().unwrap_or(0),
                    item["input_tokens"].as_i64().unwrap_or(0),
                    item["output_tokens"].as_i64().unwrap_or(0),
                    item["requested_model"].as_str().unwrap_or("—"),
                );
            }
        }
    }
    Ok(())
}
