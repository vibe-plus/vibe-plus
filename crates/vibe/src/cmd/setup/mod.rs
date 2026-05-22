//! Versioned "setup steps" — one-off (or per-version) actions the user should
//! run after install or upgrade. Examples: import from CC Switch, register
//! login-item.
//!
//! Each step declares its own `recurrence` (`OnceEver` vs `PerVersion`) and a
//! `check` function that decides whether the step is currently *applicable*.
//! Completion is recorded in `~/.vibe/state/setup.json`. The greeter shown by
//! `vibe up` lists only steps that are both applicable and not yet completed.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod autostart_step;
mod ccswitch_import;

/// Pick the Chinese or English string based on the user's detected locale.
fn pick<'a>(zh: &'a str, en: &'a str) -> &'a str {
    let locale = vibe_i18n::detect_locale_from_env();
    let lang = locale.language.as_str();
    if lang == "zh" {
        zh
    } else {
        en
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Recurrence {
    OnceEver,
    PerVersion,
}

pub enum CheckResult {
    /// Step is applicable and not yet completed. `reason` is a short Chinese hint
    /// shown in the greeter, e.g. "在 CC Switch 中发现 8 个供应商".
    Pending { reason_zh: String, reason_en: String },
    /// Already done according to state file (and, where cheap, verified live).
    Completed,
    /// Not applicable — nothing to import, nothing to set up. Skipped silently.
    NotApplicable,
}

pub struct StepOutcome {
    pub summary_zh: String,
    pub summary_en: String,
}

pub struct SetupStep {
    pub id: &'static str,
    pub title_zh: &'static str,
    pub title_en: &'static str,
    pub recurrence: Recurrence,
    pub check: fn() -> CheckResult,
    pub run: fn() -> Result<StepOutcome>,
}

pub fn registry() -> Vec<SetupStep> {
    vec![
        SetupStep {
            id: "cc-switch-import",
            title_zh: "从 CC Switch 一键迁移供应商与凭证",
            title_en: "Import providers + credentials from CC Switch",
            recurrence: Recurrence::OnceEver,
            check: ccswitch_import::check,
            run: ccswitch_import::run,
        },
        SetupStep {
            id: "autostart",
            title_zh: "注册开机自启（被 kill / 重启后自动恢复）",
            title_en: "Register login-item / autostart",
            recurrence: Recurrence::OnceEver,
            check: autostart_step::check,
            run: autostart_step::run,
        },
    ]
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SetupState {
    #[serde(default)]
    pub steps: HashMap<String, StepRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub completed_at: String,
    pub vibe_version: String,
    pub recurrence: Recurrence,
}

fn read_state() -> Result<SetupState> {
    let path = vibe_core::paths::setup_state_path()?;
    if !path.exists() {
        return Ok(SetupState::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(SetupState::default());
    }
    let state = serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", path.display()))?;
    Ok(state)
}

fn write_state(state: &SetupState) -> Result<()> {
    let path = vibe_core::paths::setup_state_path()?;
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Returns true if the state file says this step is done *for this version's
/// recurrence rules*. PerVersion steps reset on every CLI bump.
fn is_recorded_done(state: &SetupState, step: &SetupStep) -> bool {
    let Some(rec) = state.steps.get(step.id) else {
        return false;
    };
    match step.recurrence {
        Recurrence::OnceEver => true,
        Recurrence::PerVersion => rec.vibe_version == vibe_core::VERSION,
    }
}

fn record_done(step: &SetupStep) -> Result<()> {
    let mut state = read_state().unwrap_or_default();
    state.steps.insert(
        step.id.to_string(),
        StepRecord {
            completed_at: chrono::Utc::now().to_rfc3339(),
            vibe_version: vibe_core::VERSION.to_string(),
            recurrence: step.recurrence,
        },
    );
    write_state(&state)
}

pub struct PendingStep {
    pub id: &'static str,
    pub title: &'static str,
    pub reason: String,
}

/// Steps that should appear in the greeter: applicable + not recorded as done.
pub fn pending_steps() -> Vec<PendingStep> {
    let state = read_state().unwrap_or_default();
    let mut out = Vec::new();
    for step in registry() {
        if is_recorded_done(&state, &step) {
            continue;
        }
        match (step.check)() {
            CheckResult::Pending { reason_zh, reason_en } => out.push(PendingStep {
                id: step.id,
                title: pick(step.title_zh, step.title_en),
                reason: pick(&reason_zh, &reason_en).to_string(),
            }),
            CheckResult::Completed | CheckResult::NotApplicable => {}
        }
    }
    out
}

/// Print a short hint when there are pending setup steps. Called from `vibe up`.
pub fn print_greeter_hint() {
    let pending = pending_steps();
    if pending.is_empty() {
        return;
    }
    let header = pick(
        &format!("[setup] {} 项 setup 待办（vibe setup 查看 / 执行）：", pending.len()),
        &format!("[setup] {} pending step(s) — run `vibe setup` to inspect or execute:", pending.len()),
    )
    .to_string();
    println!("  {header}");
    for p in &pending {
        println!("    · {}（{}） — {}", p.title, p.id, p.reason);
    }
}

pub fn run_step(id: &str) -> Result<()> {
    let Some(step) = registry().into_iter().find(|s| s.id == id) else {
        anyhow::bail!("unknown setup step: {id} (use `vibe setup status` for the list)");
    };
    println!("→ {}（{}）", pick(step.title_zh, step.title_en), step.id);
    let outcome = (step.run)().with_context(|| format!("run setup step {id}"))?;
    println!("  ✓ {}", pick(&outcome.summary_zh, &outcome.summary_en));
    record_done(&step)
}

pub fn run_all_pending() -> Result<()> {
    let pending = pending_steps();
    if pending.is_empty() {
        println!(
            "{}",
            pick("没有待办的 setup 步骤。", "No pending setup steps.")
        );
        return Ok(());
    }
    for p in pending {
        run_step(p.id)?;
    }
    Ok(())
}

pub fn print_status() -> Result<()> {
    let state = read_state().unwrap_or_default();
    println!("setup steps:");
    for step in registry() {
        let done = is_recorded_done(&state, &step);
        let check = (step.check)();
        let status = match (&check, done) {
            (_, true) => "✓ done",
            (CheckResult::NotApplicable, _) => "– n/a",
            (CheckResult::Pending { .. }, _) => "○ pending",
            (CheckResult::Completed, _) => "✓ already in place",
        };
        let recurrence = match step.recurrence {
            Recurrence::OnceEver => "once-ever",
            Recurrence::PerVersion => "per-version",
        };
        let title = pick(step.title_zh, step.title_en);
        println!("  {status:<24} {} [{recurrence}]", step.id);
        println!("                          {title}");
        if let CheckResult::Pending { reason_zh, reason_en } = &check {
            println!("                          → {}", pick(reason_zh, reason_en));
        }
        if let Some(rec) = state.steps.get(step.id) {
            println!(
                "                          last run: {} (vibe {})",
                rec.completed_at, rec.vibe_version
            );
        }
    }
    Ok(())
}
