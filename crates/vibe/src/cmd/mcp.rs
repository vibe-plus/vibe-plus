use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use reqwest::Url;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, JsonObject, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
};
use rmcp::ErrorData as McpError;
use rmcp::ServiceExt;
use serde::Deserialize;
use serde_json::json;
use std::borrow::Cow;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use toml_edit::{value, DocumentMut, Item, Table};

const DEFAULT_MCP_SERVER_NAME: &str = "vibe_plus";
const DEFAULT_STARTUP_TIMEOUT_SEC: f64 = 10.0;
const DEFAULT_TOOL_TIMEOUT_SEC: f64 = 120.0;
const DEFAULT_WEBSITE_URL: &str = "https://web.vibe-plus.localhost";

#[derive(Args)]
pub struct McpArgs {
    #[command(subcommand)]
    pub command: McpCmd,
}

#[derive(Subcommand)]
pub enum McpCmd {
    /// Run the Vibe Plus MCP server over stdio.
    Serve,
    /// Install the Vibe Plus MCP server into Codex config.
    Install(InstallArgs),
}

#[derive(Args, Clone)]
pub struct InstallArgs {
    /// MCP server name in Codex config.
    #[arg(long, default_value = DEFAULT_MCP_SERVER_NAME)]
    pub name: String,
    /// Override CODEX_HOME. Defaults to ~/.codex.
    #[arg(long)]
    pub codex_home: Option<PathBuf>,
}

pub async fn run(args: McpArgs) -> Result<()> {
    match args.command {
        McpCmd::Serve => serve().await,
        McpCmd::Install(args) => install(args),
    }
}

async fn serve() -> Result<()> {
    let service = VibePlusMcpServer::new()?;
    let running = service
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    running.waiting().await?;
    Ok(())
}

fn install(args: InstallArgs) -> Result<()> {
    let codex_home = args.codex_home.unwrap_or_else(default_codex_home);
    let config_path = codex_home.join("config.toml");

    let raw = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut doc = if raw.trim().is_empty() {
        DocumentMut::new()
    } else {
        raw.parse::<DocumentMut>()
            .context("invalid TOML in Codex config")?
    };

    let root = doc.as_table_mut();
    let servers_item = root
        .entry("mcp_servers")
        .or_insert(Item::Table(Table::new()));
    let servers = servers_item
        .as_table_mut()
        .context("mcp_servers must be a TOML table")?;

    let mut entry = Table::new();
    entry.insert("command", value("cargo"));
    let mut args_array = toml_edit::Array::default();
    args_array.push("run");
    args_array.push("-q");
    args_array.push("-p");
    args_array.push("vibe");
    args_array.push("--");
    args_array.push("mcp");
    args_array.push("serve");
    entry.insert("args", Item::Value(args_array.into()));
    let vibe_root = std::env::current_dir().context("failed to determine Vibe Plus root")?;
    entry.insert("cwd", value(vibe_root.display().to_string()));
    entry.insert("enabled", value(true));
    entry.insert("supports_parallel_tool_calls", value(false));
    entry.insert("startup_timeout_sec", value(DEFAULT_STARTUP_TIMEOUT_SEC));
    entry.insert("tool_timeout_sec", value(DEFAULT_TOOL_TIMEOUT_SEC));

    let mut tools = Table::new();
    for name in ["open_logs", "get_log_overview", "restart_gateway"] {
        let mut tool = Table::new();
        tool.insert(
            "approval_mode",
            value(if name == "restart_gateway" {
                "prompt"
            } else {
                "auto"
            }),
        );
        tools.insert(name, Item::Table(tool));
    }

    entry.insert("tools", Item::Table(tools));
    servers.insert(&args.name, Item::Table(entry));

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, doc.to_string())?;

    println!(
        "Installed MCP server `{}` into {}",
        args.name,
        config_path.display()
    );
    println!("  command: cargo");
    println!("  args: run -q -p vibe -- mcp serve");
    println!("  tools: open_logs (auto), get_log_overview (auto), restart_gateway (prompt)");
    Ok(())
}

fn default_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            directories::UserDirs::new()
                .map(|dirs| dirs.home_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".codex")
        })
}

#[derive(Clone)]
struct VibePlusMcpServer {
    tools: Arc<Vec<Tool>>,
    base_url: String,
    website_url: String,
    vibe_root: PathBuf,
}

impl VibePlusMcpServer {
    fn new() -> Result<Self> {
        Ok(Self {
            tools: Arc::new(vec![
                Self::open_logs_tool(),
                Self::get_log_overview_tool(),
                Self::restart_gateway_tool(),
            ]),
            base_url: super::configured_base_url()?,
            website_url: std::env::var("VIBE_PLUS_WEBSITE_URL")
                .unwrap_or_else(|_| DEFAULT_WEBSITE_URL.to_string()),
            vibe_root: std::env::current_dir().context("failed to determine current directory")?,
        })
    }

    fn open_logs_tool() -> Tool {
        let schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Optional request log id to deep-link in the Vibe Plus logs page." },
                "provider_id": { "type": "string" },
                "status": { "type": "string", "enum": ["all", "ok", "error"] },
                "hours": { "type": "integer", "minimum": 1, "maximum": 168 },
                "view": { "type": "string", "enum": ["all", "codex", "claude"] }
            },
            "additionalProperties": false
        }))
        .expect("open_logs schema should deserialize");
        let mut tool = Tool::new(
            Cow::Borrowed("open_logs"),
            Cow::Borrowed("Return Vibe Plus logs page URLs and API endpoints so the agent can inspect logs through the web console or fetch raw JSON directly."),
            Arc::new(schema),
        );
        tool.annotations = Some(ToolAnnotations::new().read_only(true));
        tool
    }

    fn get_log_overview_tool() -> Tool {
        let schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Optional request log id to include direct API links for a single request." },
                "provider_id": { "type": "string" },
                "status": { "type": "string", "enum": ["ok", "error"] },
                "hours": { "type": "integer", "minimum": 1, "maximum": 168 },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
            },
            "additionalProperties": false
        }))
        .expect("get_log_overview schema should deserialize");
        let mut tool = Tool::new(
            Cow::Borrowed("get_log_overview"),
            Cow::Borrowed("Fetch a lightweight recent-log overview plus direct URLs for deeper inspection, without trying to fully analyze the logs in-tool."),
            Arc::new(schema),
        );
        tool.annotations = Some(ToolAnnotations::new().read_only(true));
        tool
    }

    fn restart_gateway_tool() -> Tool {
        let schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "reason": { "type": "string", "description": "Optional human-readable reason for the restart." }
            },
            "additionalProperties": false
        }))
        .expect("restart_gateway schema should deserialize");
        Tool::new(
            Cow::Borrowed("restart_gateway"),
            Cow::Borrowed("Restart the Vibe Plus gateway by running `bun gateway:restart` in the installed Vibe Plus directory."),
            Arc::new(schema),
        )
    }

    fn parse_args<T: for<'de> Deserialize<'de>>(
        request: &CallToolRequestParams,
        tool_name: &'static str,
    ) -> Result<T, McpError> {
        let value = serde_json::Value::Object(
            request
                .arguments
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect(),
        );
        serde_json::from_value(value).map_err(|err| {
            McpError::invalid_params(format!("invalid arguments for {tool_name}: {err}"), None)
        })
    }

    fn logs_api_url(&self, args: &LogOverviewArgs) -> Result<Url, McpError> {
        let limit = args.limit.unwrap_or(20).clamp(1, 100);
        let mut url = Url::parse(&format!("{}/_vp/logs", self.base_url))
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("limit", &limit.to_string());
            if let Some(status) = args.status.as_deref() {
                q.append_pair("status", status);
            }
            if let Some(provider_id) = args.provider_id.as_deref().filter(|s| !s.is_empty()) {
                q.append_pair("provider_id", provider_id);
            }
            if let Some(hours) = args.hours {
                let since = chrono::Utc::now().timestamp() - i64::from(hours) * 3600;
                q.append_pair("since", &since.to_string());
            }
        }
        Ok(url)
    }

    async fn fetch_logs_page(&self, args: &LogOverviewArgs) -> Result<serde_json::Value, McpError> {
        let url = self.logs_api_url(args)?;
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|err| McpError::internal_error(err.to_string(), None))?
            .get(url)
            .send()
            .await
            .map_err(|err| McpError::internal_error(format!("request failed: {err}"), None))?
            .error_for_status()
            .map_err(|err| McpError::internal_error(format!("gateway error: {err}"), None))?
            .json::<serde_json::Value>()
            .await
            .map_err(|err| McpError::internal_error(format!("invalid JSON: {err}"), None))
    }

    fn website_logs_url(&self, args: &OpenLogsArgs) -> String {
        let mut url = format!("{}/ui/monitor", self.website_url.trim_end_matches('/'));
        let mut query = vec![];
        if let Some(view) = args.view.as_deref().filter(|v| *v != "all") {
            query.push(format!("view={view}"));
        }
        if let Some(status) = args.status.as_deref().filter(|v| *v != "all") {
            query.push(format!("status={status}"));
        }
        if let Some(provider_id) = args.provider_id.as_deref().filter(|s| !s.is_empty()) {
            query.push(format!("provider_id={provider_id}"));
        }
        if let Some(hours) = args.hours {
            query.push(format!("hours={hours}"));
        }
        if let Some(id) = args.id.as_deref().filter(|s| !s.is_empty()) {
            query.push(format!("id={id}"));
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        url
    }

    async fn open_logs(&self, args: OpenLogsArgs) -> Result<serde_json::Value, McpError> {
        let list_args = LogOverviewArgs {
            id: args.id.clone(),
            provider_id: args.provider_id.clone(),
            status: args
                .status
                .clone()
                .and_then(|s| if s == "all" { None } else { Some(s) }),
            hours: args.hours,
            limit: Some(20),
        };
        let api_list_url = self.logs_api_url(&list_args)?.to_string();
        let website_logs_url = self.website_logs_url(&args);
        let detail = args.id.as_ref().map(|id| {
            json!({
                "detail_api_url": format!("{}/_vp/logs/{}", self.base_url, id),
                "attempts_api_url": format!("{}/_vp/logs/{}/attempts", self.base_url, id),
                "stream_trace_api_url": format!("{}/_vp/logs/{}/stream-trace", self.base_url, id),
            })
        });
        Ok(json!({
            "website_logs_url": website_logs_url,
            "list_api_url": api_list_url,
            "filters": args,
            "detail": detail,
            "note": "Use website_logs_url for the interactive console, or fetch the API URLs directly for raw JSON."
        }))
    }

    async fn get_log_overview(&self, args: LogOverviewArgs) -> Result<serde_json::Value, McpError> {
        let page = self.fetch_logs_page(&args).await?;
        let items = page
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let summary = items
            .into_iter()
            .take(args.limit.unwrap_or(20).clamp(1, 100) as usize)
            .map(|item| {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                json!({
                    "id": if id.is_empty() { serde_json::Value::Null } else { json!(id) },
                    "provider_id": item.get("provider_id").cloned().unwrap_or(serde_json::Value::Null),
                    "requested_model": item.get("requested_model").cloned().unwrap_or(serde_json::Value::Null),
                    "upstream_model": item.get("upstream_model").cloned().unwrap_or(serde_json::Value::Null),
                    "status_code": item.get("status_code").cloned().unwrap_or(serde_json::Value::Null),
                    "latency_ms": item.get("latency_ms").cloned().unwrap_or(serde_json::Value::Null),
                    "error": item.get("error").cloned().unwrap_or(serde_json::Value::Null),
                    "started_at": item.get("started_at").cloned().unwrap_or(serde_json::Value::Null),
                    "detail_api_url": if id.is_empty() { serde_json::Value::Null } else { json!(format!("{}/_vp/logs/{}", self.base_url, id)) },
                    "attempts_api_url": if id.is_empty() { serde_json::Value::Null } else { json!(format!("{}/_vp/logs/{}/attempts", self.base_url, id)) },
                    "stream_trace_api_url": if id.is_empty() { serde_json::Value::Null } else { json!(format!("{}/_vp/logs/{}/stream-trace", self.base_url, id)) },
                })
            })
            .collect::<Vec<_>>();

        let mut open_args = OpenLogsArgs {
            id: args.id.clone(),
            provider_id: args.provider_id.clone(),
            status: args.status.clone(),
            hours: args.hours,
            view: None,
        };
        if open_args.status.is_none() {
            open_args.status = Some("all".to_string());
        }

        Ok(json!({
            "filters": args,
            "website_logs_url": self.website_logs_url(&open_args),
            "list_api_url": self.logs_api_url(&LogOverviewArgs { limit: args.limit, ..args.clone() })?.to_string(),
            "items": summary,
            "note": "This tool intentionally returns a lightweight overview plus direct URLs. Let the model inspect the linked JSON or the website UI for deeper reasoning."
        }))
    }

    async fn restart_gateway(
        &self,
        args: RestartGatewayArgs,
    ) -> Result<serde_json::Value, McpError> {
        let output = Command::new("bun")
            .arg("gateway:restart")
            .current_dir(&self.vibe_root)
            .output()
            .map_err(|err| {
                McpError::internal_error(format!("failed to launch bun: {err}"), None)
            })?;

        Ok(json!({
            "ok": output.status.success(),
            "reason": args.reason,
            "cwd": self.vibe_root,
            "status": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }))
    }
}

impl ServerHandler for VibePlusMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Use open_logs to get logs-page and API URLs, get_log_overview for a light recent snapshot with deep links, and restart_gateway only when an operator explicitly wants a restart.".to_string(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
            ..ServerInfo::default()
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = self.tools.clone();
        async move {
            Ok(ListToolsResult {
                tools: (*tools).clone(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let result = match request.name.as_ref() {
            "open_logs" => {
                let args = Self::parse_args::<OpenLogsArgs>(&request, "open_logs")?;
                self.open_logs(args).await?
            }
            "get_log_overview" => {
                let args = Self::parse_args::<LogOverviewArgs>(&request, "get_log_overview")?;
                self.get_log_overview(args).await?
            }
            "restart_gateway" => {
                let args = Self::parse_args::<RestartGatewayArgs>(&request, "restart_gateway")?;
                self.restart_gateway(args).await?
            }
            other => {
                return Err(McpError::invalid_params(
                    format!("unknown tool: {other}"),
                    None,
                ))
            }
        };

        Ok(CallToolResult {
            content: vec![],
            structured_content: Some(result),
            is_error: Some(false),
            meta: None,
        })
    }
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct OpenLogsArgs {
    id: Option<String>,
    provider_id: Option<String>,
    status: Option<String>,
    hours: Option<i32>,
    view: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct LogOverviewArgs {
    id: Option<String>,
    provider_id: Option<String>,
    status: Option<String>,
    hours: Option<i32>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RestartGatewayArgs {
    reason: Option<String>,
}
