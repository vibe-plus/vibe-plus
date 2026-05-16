//! Generates TS type files into `packages/protocol/types/`.
//!
//! Run from the workspace root: `cargo run -p vibe-protocol --bin export-types`.

use std::{
    env, fs,
    path::{Path, PathBuf},
};
use ts_rs::TS;
use vibe_protocol::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Provider::export_all()?;
    ProviderProtocol::export_all()?;
    RemoteDetectedProtocol::export_all()?;
    ProviderSpeedtestResult::export_all()?;
    ModelAlias::export_all()?;
    ProviderKind::export_all()?;
    Route::export_all()?;
    RouteInput::export_all()?;
    RouteTier::export_all()?;
    ForwardStrategy::export_all()?;
    RequestLog::export_all()?;
    ModelPricing::export_all()?;
    Meta::export_all()?;
    WebCompatibility::export_all()?;
    Status::export_all()?;
    UsageSummary::export_all()?;
    ClientStatus::export_all()?;
    ClientTakeoverResult::export_all()?;
    CodexAppProcess::export_all()?;
    CodexAppStatus::export_all()?;
    CodexAppActionResult::export_all()?;
    WsEvent::export_all()?;
    ProvidersOverviewStreamStarted::export_all()?;
    ProvidersOverviewProvidersChunk::export_all()?;
    ProvidersOverviewHealthChunk::export_all()?;
    ProvidersOverviewPoolsChunk::export_all()?;
    ProvidersOverviewCredentialsChunk::export_all()?;
    ProvidersOverviewCodexPlansChunk::export_all()?;
    ProvidersOverviewStreamEnded::export_all()?;
    RequestActivity::export_all()?;
    UpstreamAttemptPhase::export_all()?;
    UpstreamAttemptOutcome::export_all()?;
    UpstreamAttemptActivity::export_all()?;
    RequestRuntimeStats::export_all()?;
    UpstreamAttemptLog::export_all()?;
    LogPage::export_all()?;
    Health::export_all()?;
    ProviderInput::export_all()?;
    ProviderSpeedtestInput::export_all()?;
    ProvidersOverview::export_all()?;
    CredentialPoolStatus::export_all()?;
    ProviderAuthPoolSummary::export_all()?;
    ProviderHealthSummary::export_all()?;
    CredentialPlanSnapshot::export_all()?;
    ProviderCodexPlanItem::export_all()?;
    CodexPlanRefreshResult::export_all()?;
    ModelStat::export_all()?;
    ProviderStat::export_all()?;
    DashboardStats::export_all()?;
    Credential::export_all()?;
    for dir in protocol_type_dirs() {
        fix_nodenext_imports(&dir)?;
    }
    println!("exported types to {}", vibe_protocol::ts_out_dir());
    Ok(())
}

fn protocol_type_dirs() -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    vec![
        manifest_dir.join(vibe_protocol::ts_out_dir()),
        manifest_dir.join("packages/protocol/types"),
        workspace_dir.join("packages/protocol/types"),
        workspace_dir.join("crates/packages/protocol/types"),
    ]
}

fn fix_nodenext_imports(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("ts") {
            continue;
        }

        let source = fs::read_to_string(&path)?;
        let fixed = source
            .lines()
            .map(fix_nodenext_import_line)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        if fixed != source {
            fs::write(path, fixed)?;
        }
    }
    Ok(())
}

fn fix_nodenext_import_line(line: &str) -> String {
    let Some((prefix, import_path)) = line.split_once(" from \"./") else {
        return line.to_owned();
    };
    if !prefix.trim_start().starts_with("import type ") || import_path.ends_with(".js\";") {
        return line.to_owned();
    }
    let Some(module) = import_path.strip_suffix("\";") else {
        return line.to_owned();
    };
    format!("{prefix} from \"./{module}.js\";")
}
