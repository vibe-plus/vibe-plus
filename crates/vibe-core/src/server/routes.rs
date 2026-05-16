use super::*;

pub(super) async fn list_routes(
    State(state): State<AppState>,
) -> Result<Json<Vec<vibe_protocol::Route>>, AppError> {
    let routes = run_blocking(state, |s| s.db.route_list()).await?;
    Ok(Json(routes))
}

pub(super) async fn create_route(
    State(state): State<AppState>,
    Json(input): Json<vibe_protocol::RouteInput>,
) -> Result<Json<vibe_protocol::Route>, AppError> {
    let route = run_blocking(state.clone(), move |s| s.db.route_insert(input)).await?;
    publish_routes_changed_soon(state);
    Ok(Json(route))
}

pub(super) async fn update_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<vibe_protocol::RouteInput>,
) -> Result<Json<vibe_protocol::Route>, AppError> {
    let route = run_blocking(state.clone(), move |s| s.db.route_update(&id, input)).await?;
    publish_routes_changed_soon(state);
    Ok(Json(route))
}

pub(super) async fn delete_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    run_blocking(state.clone(), move |s| s.db.route_delete(&id)).await?;
    publish_routes_changed_soon(state);
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn publish_routes_changed_soon(state: AppState) {
    tokio::spawn(async move {
        match run_blocking(state.clone(), |s| s.db.route_list()).await {
            Ok(routes) => state.ws.publish(WsEvent::RoutesChanged { routes }),
            Err(e) => tracing::warn!(?e, "build routes ws event failed"),
        }
    });
}

#[derive(Debug, Deserialize)]
pub(super) struct RouteExplainQuery {
    model: String,
    wire: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct RouteExplainPick {
    provider_id: String,
    provider_name: String,
    provider_kind: String,
    upstream_model: String,
    priority: i32,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct RouteExplainResponse {
    requested_model: String,
    wire: String,
    matched_route: Option<vibe_protocol::Route>,
    candidates: Vec<RouteExplainPick>,
}

pub(super) async fn explain_route(
    State(state): State<AppState>,
    Query(q): Query<RouteExplainQuery>,
) -> Result<Json<RouteExplainResponse>, AppError> {
    let wire = wire_from_str(q.wire.as_deref().unwrap_or("openai-responses"))?;
    let (providers, routes) = run_blocking(state, |s| {
        Ok::<_, anyhow::Error>((s.db.provider_list()?, s.db.route_list()?))
    })
    .await?;
    let (matched_route, candidates) =
        router::candidates_with_routes(&providers, &routes, wire, &q.model);
    Ok(Json(RouteExplainResponse {
        requested_model: q.model,
        wire: wire_name(wire).into(),
        matched_route,
        candidates: candidates
            .into_iter()
            .map(|p| RouteExplainPick {
                provider_id: p.provider.id,
                provider_name: p.provider.name,
                provider_kind: format!("{:?}", p.provider.kind),
                upstream_model: p.upstream_model,
                priority: p.provider.priority,
            })
            .collect(),
    }))
}

pub(super) fn wire_from_str(s: &str) -> Result<Wire, AppError> {
    Ok(match s {
        "anthropic" => Wire::Anthropic,
        "openai-chat" => Wire::OpenaiChat,
        "openai-responses" => Wire::OpenaiResponses,
        "gemini-native" => Wire::GeminiNative,
        other => {
            return Err(anyhow::anyhow!(
                "unknown wire {other}; expected anthropic, openai-chat, openai-responses, gemini-native"
            )
            .into())
        }
    })
}

pub(super) fn wire_name(wire: Wire) -> &'static str {
    match wire {
        Wire::Anthropic => "anthropic",
        Wire::OpenaiChat => "openai-chat",
        Wire::OpenaiResponses => "openai-responses",
        Wire::GeminiNative => "gemini-native",
    }
}
