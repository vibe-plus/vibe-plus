use super::*;

pub(super) async fn scan_import_local(
    State(_state): State<AppState>,
) -> Result<Json<Vec<LocalCandidate>>, AppError> {
    let items = crate::provider_import::scan_local_candidates()?;
    Ok(Json(items))
}

pub(super) async fn import_local(
    State(state): State<AppState>,
    Json(clients): Json<Vec<String>>,
) -> Result<Json<Vec<Provider>>, AppError> {
    let imported = run_blocking(state, move |s| {
        crate::provider_import::import_local_clients(&s.db, &clients)
    })
    .await?;
    Ok(Json(imported))
}
