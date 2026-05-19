use super::*;
use crate::config::CodexSummaryConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexGatewaySettings {
    pub summary: CodexSummaryConfig,
    pub route_status_enabled: bool,
}

pub(super) async fn get_codex_gateway_settings(
    State(state): State<AppState>,
) -> Json<CodexGatewaySettings> {
    Json(CodexGatewaySettings {
        summary: state.codex_summary_config(),
        route_status_enabled: state.codex_route_status_enabled(),
    })
}

pub(super) async fn put_codex_gateway_settings(
    State(state): State<AppState>,
    Json(input): Json<CodexGatewaySettings>,
) -> Json<CodexGatewaySettings> {
    state.set_codex_summary_config(input.summary.clone());
    state.set_codex_route_status_enabled(input.route_status_enabled);
    Json(CodexGatewaySettings {
        summary: state.codex_summary_config(),
        route_status_enabled: state.codex_route_status_enabled(),
    })
}
