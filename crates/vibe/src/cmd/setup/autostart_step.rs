//! Setup step: register login-item autostart. Wraps `cmd::autostart::enable`.

use super::{CheckResult, StepOutcome};
use crate::cmd::autostart;
use anyhow::Result;

pub fn check() -> CheckResult {
    // If autostart is already live, mark Completed so the greeter stops nagging.
    match autostart::ensure_enabled() {
        Ok(autostart::EnsureOutcome::Registered { .. })
        | Ok(autostart::EnsureOutcome::AlreadyRegistered) => CheckResult::Completed,
        Ok(autostart::EnsureOutcome::UserDisabled) => CheckResult::NotApplicable,
        Ok(autostart::EnsureOutcome::Skipped(_)) => CheckResult::NotApplicable,
        Err(_) => CheckResult::Pending {
            reason_zh: "未注册（注册后被 kill 或重启都会自动恢复）".into(),
            reason_en: "not registered (will auto-restart on kill/reboot once enabled)".into(),
        },
    }
}

pub fn run() -> Result<StepOutcome> {
    autostart::enable()?;
    Ok(StepOutcome {
        summary_zh: "开机自启已注册".into(),
        summary_en: "autostart registered".into(),
    })
}
