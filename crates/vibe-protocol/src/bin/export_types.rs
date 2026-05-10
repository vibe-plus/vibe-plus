//! Generates TS type files into `packages/protocol/types/`.
//!
//! Run from the workspace root: `cargo run -p vibe-protocol --bin export-types`.

use ts_rs::TS;
use vibe_protocol::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Provider::export_all()?;
    ModelAlias::export_all()?;
    ProviderKind::export_all()?;
    Route::export_all()?;
    RouteTier::export_all()?;
    RequestLog::export_all()?;
    ModelPricing::export_all()?;
    Status::export_all()?;
    UsageSummary::export_all()?;
    WsEvent::export_all()?;
    LogPage::export_all()?;
    Health::export_all()?;
    ProviderInput::export_all()?;
    ProviderHealthSummary::export_all()?;
    CredentialPlanSnapshot::export_all()?;
    ProviderCodexPlanItem::export_all()?;
    CodexPlanRefreshResult::export_all()?;
    println!("exported types to {}", vibe_protocol::ts_out_dir());
    Ok(())
}
