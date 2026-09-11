use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub const UNIT: &str = "83sc-kbd-idle";
pub const DEFAULT_SECS: i32 = 5;

fn conf_path() -> PathBuf {
    crate::profiles::config_path().with_file_name("kbd-idle.conf")
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| format!("systemctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn enabled() -> bool {
    systemctl(&["is-enabled", "--quiet", UNIT]).is_ok()
}

pub fn timeout_secs() -> i32 {
    fs::read_to_string(conf_path())
        .ok()
        .and_then(|s| {
            s.lines()
                .filter_map(|l| l.split_once('='))
                .find(|(k, _)| k.trim() == "timeout")
                .and_then(|(_, v)| v.trim().parse().ok())
        })
        .unwrap_or(DEFAULT_SECS)
}

/// The daemon only reads its config at startup, so a timeout change is a
/// restart. Enabling and disabling go through systemd so the state survives
/// logout the same way it would if set from the shell.
pub fn apply(on: bool, secs: i32) -> Result<(), String> {
    let p = conf_path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&p, format!("timeout={secs}\n")).map_err(|e| e.to_string())?;
    if on {
        systemctl(&["enable", "--now", UNIT])?;
        systemctl(&["restart", UNIT])
    } else {
        systemctl(&["disable", "--now", UNIT])
    }
}
