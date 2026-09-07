use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// (trip temperature C, fan rpm) per point, lowest first
    pub curve: Vec<(i32, i32)>,
    pub pl1: i32,
    pub pl2: i32,
    pub tau: i32,
    /// EC power mode to switch to when this profile is applied.
    /// 1 quiet, 2 balanced, 3 performance, 224 extreme, 255 custom.
    /// 0 means leave the current mode alone.
    pub powermode: i32,
    pub builtin: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub profiles: Vec<Profile>,
    /// EC power mode -> profile name. When the mode changes, apply that profile.
    pub bindings: Vec<(i32, String)>,
    pub apply_on_start: Option<String>,
}

pub fn builtins() -> Vec<Profile> {
    vec![
        Profile {
            name: "Silent".into(),
            curve: vec![(55, 1000), (62, 1300), (68, 1600), (72, 2000), (76, 2400),
                        (80, 2900), (84, 3400), (87, 3900), (90, 4300), (94, 4300)],
            pl1: 30, pl2: 55, tau: 8, powermode: 1, builtin: true,
        },
        Profile {
            name: "Balanced".into(),
            curve: vec![(50, 1200), (55, 1500), (60, 1900), (65, 2300), (70, 2700),
                        (75, 3100), (80, 3500), (85, 3900), (88, 4300), (92, 4300)],
            pl1: 40, pl2: 75, tau: 8, powermode: 2, builtin: true,
        },
        Profile {
            name: "Gaming".into(),
            curve: vec![(45, 1500), (50, 2000), (55, 2500), (60, 3000), (65, 3400),
                        (70, 3800), (75, 4100), (80, 4300), (85, 4300), (90, 4300)],
            pl1: 45, pl2: 90, tau: 8, powermode: 255, builtin: true,
        },
        Profile {
            name: "Max cooling".into(),
            curve: vec![(40, 2000), (45, 2600), (50, 3200), (55, 3700), (60, 4100),
                        (65, 4300), (70, 4300), (75, 4300), (80, 4300), (85, 4300)],
            pl1: 45, pl2: 90, tau: 8, powermode: 255, builtin: true,
        },
    ]
}

pub fn config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(".config")
        });
    base.join("83sc-control").join("profiles.json")
}

pub fn load() -> Config {
    let p = config_path();
    let mut cfg: Config = fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    // Builtins are code, not data: refresh them on every load so an old config
    // file never pins a stale curve, while user profiles are left untouched.
    cfg.profiles.retain(|x| !x.builtin);
    let mut all = builtins();
    all.extend(cfg.profiles);
    cfg.profiles = all;
    cfg
}

pub fn save(cfg: &Config) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut out = cfg.clone();
    out.profiles.retain(|x| !x.builtin);
    fs::write(&p, serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub fn mode_name(mode: i32) -> &'static str {
    match mode {
        1 => "Quiet",
        2 => "Balanced",
        3 => "Performance",
        224 => "Extreme",
        255 => "Custom",
        _ => "Unknown",
    }
}

pub const MODES: [i32; 5] = [1, 2, 3, 224, 255];
