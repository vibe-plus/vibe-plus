use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct LogsArgs {
    #[command(subcommand)]
    pub cmd: Option<LogsCmd>,
    /// Follow new entries as they arrive.
    #[arg(long, short)]
    pub tail: bool,
    /// How many recent entries to show.
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
}

#[derive(Subcommand)]
pub enum LogsCmd {
    /// Show one full request log row.
    Show { id: String },
    /// Show stream disconnect diagnostics for one log row.
    Trace { id: String },
}

pub async fn run(args: LogsArgs) -> Result<()> {
    if let Some(cmd) = args.cmd {
        return match cmd {
            LogsCmd::Show { id } => show(&id).await,
            LogsCmd::Trace { id } => trace(&id).await,
        };
    }

    let base = super::configured_base_url()?;

    if args.tail {
        println!("Connecting to live log stream (Ctrl-C to stop)…");
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;
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
            println!(
                "{:<24}  {:>6}  {:>8}ms  {:>6}in  {:>6}out  model",
                "id", "status", "latency", "tokens", "tokens"
            );
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

async fn show(id: &str) -> Result<()> {
    let v = get_json(&format!("/_vp/logs/{id}")).await?;
    println!("id:              {}", v["id"].as_str().unwrap_or(id));
    println!("started_at:      {}", v["started_at"]);
    println!("app:             {}", str_or_dash(&v["app"]));
    println!("provider:        {}", str_or_dash(&v["provider_id"]));
    println!("model:           {}", str_or_dash(&v["requested_model"]));
    println!("upstream_model:  {}", str_or_dash(&v["upstream_model"]));
    println!("status:          {}", v["status_code"]);
    println!("error:           {}", str_or_dash(&v["error"]));
    println!("latency_ms:      {}", v["latency_ms"]);
    println!("first_token_ms:  {}", v["first_token_ms"]);
    println!("stream_kind:     {}", str_or_dash(&v["stream_kind"]));
    println!("stream_reason:   {}", str_or_dash(&v["stream_end_reason"]));
    println!("terminal_seen:   {}", v["stream_terminal_seen"]);
    print_block("headers", &v["request_headers"]);
    print_block("request", &v["request_body"]);
    print_block("response", &v["response_body"]);
    print_block("client", &v["client_response_body"]);
    Ok(())
}

async fn trace(id: &str) -> Result<()> {
    let v = get_json(&format!("/_vp/logs/{id}/stream-trace")).await?;
    println!("id:                  {}", v["id"].as_str().unwrap_or(id));
    println!("verdict:             {}", str_or_dash(&v["verdict"]));
    println!("stream_kind:         {}", str_or_dash(&v["stream_kind"]));
    println!("bridge_mode:         {}", str_or_dash(&v["bridge_mode"]));
    println!("terminal_seen:       {}", v["stream_terminal_seen"]);
    println!(
        "terminal_type:       {}",
        str_or_dash(&v["upstream_terminal_type"])
    );
    println!(
        "end_reason:          {}",
        str_or_dash(&v["stream_end_reason"])
    );
    println!(
        "error_detail:        {}",
        str_or_dash(&v["stream_error_detail"])
    );
    println!("upstream_first_ms:   {}", v["upstream_first_byte_ms"]);
    println!("client_first_ms:     {}", v["client_first_write_ms"]);
    println!("last_upstream_ms:    {}", v["last_upstream_event_ms"]);
    println!("last_client_ms:      {}", v["last_client_write_ms"]);
    println!(
        "upstream chunks:     {} / {} bytes",
        v["upstream_chunk_count"], v["upstream_bytes"]
    );
    println!(
        "client chunks:       {} / {} bytes",
        v["client_chunk_count"], v["client_bytes"]
    );
    println!(
        "sse events:          events={} data={} comments={} keepalive={} done={} parse_errors={}",
        v["sse_event_count"],
        v["sse_data_count"],
        v["sse_comment_count"],
        v["sse_keepalive_count"],
        v["sse_done_count"],
        v["parse_error_count"],
    );
    println!("first_keepalive_ms:  {}", v["first_keepalive_ms"]);
    println!("last_keepalive_ms:   {}", v["last_keepalive_ms"]);
    println!("last_data_ms:        {}", v["last_data_event_ms"]);
    println!(
        "max gaps:            upstream={}ms data={}ms",
        v["max_gap_between_upstream_events_ms"], v["max_gap_between_data_events_ms"]
    );
    println!("status_injected:     {}", v["status_injected"]);
    println!("terminal_injected:   {}", v["terminal_injected"]);
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

fn str_or_dash(v: &serde_json::Value) -> &str {
    v.as_str().unwrap_or("—")
}

fn print_block(label: &str, value: &serde_json::Value) {
    if let Some(s) = value.as_str().filter(|s| !s.is_empty()) {
        println!("\n--- {label} ---\n{s}");
    }
}
