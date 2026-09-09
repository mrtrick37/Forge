//! Small, honest gaming compatibility helpers for the Hub.
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct ProtonDbResult {
    pub app_id: String,
    pub tier: String,
    pub detail: String,
}

pub fn protondb_lookup(app_id: &str) -> Option<ProtonDbResult> {
    if app_id.len() > 12 || !app_id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let url = format!("https://www.protondb.com/api/v1/reports/summaries/{app_id}.json");
    let argv = [
        "curl".to_string(),
        "-fsSL".to_string(),
        "--max-time".to_string(),
        "6".to_string(),
        url,
    ];
    let output = crate::system::process::run_bounded(&argv, Duration::from_secs(8)).ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let tier = json.get("tier")?.as_str()?.to_string();
    Some(ProtonDbResult {
        app_id: app_id.to_string(),
        detail: format!("ProtonDB rating: {tier}"),
        tier,
    })
}

pub fn protondb_lookup_many(app_ids: &[String]) -> Vec<ProtonDbResult> {
    app_ids
        .iter()
        .filter_map(|id| protondb_lookup(id))
        .take(20)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct AntiCheatEntry {
    pub game: String,
    pub status: String,
    pub detail: String,
}

pub fn anti_cheat_table() -> Vec<AntiCheatEntry> {
    vec![
        AntiCheatEntry { game: "Easy Anti-Cheat".into(), status: "title-dependent".into(), detail: "Linux support must be enabled by the game developer; Proton cannot enable it globally.".into() },
        AntiCheatEntry { game: "BattlEye".into(), status: "title-dependent".into(), detail: "Some Proton titles work when the developer opts in; check the specific game.".into() },
        AntiCheatEntry { game: "Kernel anti-cheat".into(), status: "blocked".into(), detail: "Windows kernel drivers do not run through Proton.".into() },
        AntiCheatEntry { game: "Valve Anti-Cheat".into(), status: "varies".into(), detail: "Check the title's ProtonDB reports and current Steam compatibility notes.".into() },
    ]
}
