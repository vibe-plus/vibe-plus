//! Read-only extraction of [CC Switch](https://github.com/farion1231/cc-switch) data.
//!
//! Reads `~/.cc-switch/cc-switch.db` and `settings.json` without mutating CC Switch state.
//! Query logic is adapted from CC Switch `database/dao/providers.rs` and `settings.rs`.

mod convert;
mod db;
mod extract;
mod paths;
mod redact;
mod settings;
mod types;

pub use convert::{
    ccswitch_client_id, draft_from_ccswitch, parse_ccswitch_client_id, CcSwitchImportDraft,
};
pub use extract::{extract_default, extract_from_dir};
pub use paths::{cc_switch_db_path, cc_switch_settings_path, default_cc_switch_dir};
pub use redact::redact_value;
pub use types::{
    CcSwitchAppSettings, CcSwitchAppType, CcSwitchProvider, CcSwitchProxyConfig, CcSwitchSnapshot,
};
