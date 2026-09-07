use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LEGION: &str = "/sys/bus/platform/devices/PNP0C09:00";
pub const RAPL: &str = "/sys/class/powercap/intel-rapl:0";
pub const HELPER: &str = "/usr/local/lib/83sc-control/helper.py";

pub const POINTS: usize = 10;

pub fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub fn read_i32(path: &str) -> Option<i32> {
    read(path)?.parse().ok()
}

/// Resolve a hwmon directory by driver name. The hwmonN index is allocation
/// ordered and shifts between boots, so it can never be hardcoded.
pub fn hwmon(name: &str) -> Option<PathBuf> {
    let base = Path::new("/sys/class/hwmon");
    let mut entries: Vec<_> = fs::read_dir(base).ok()?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if read(p.join("name").to_str()?)?.as_str() == name {
            return Some(p);
        }
    }
    None
}

/// Every privileged write goes through the root-owned helper, which validates
/// the target path against a hardware-subsystem whitelist. The GUI itself
/// never needs root.
pub fn write(path: &str, value: impl ToString) -> Result<(), String> {
    let out = Command::new("sudo")
        .args(["-n", HELPER, "set", path, &value.to_string()])
        .output()
        .map_err(|e| format!("could not run helper: {e}"))?;
    // rc 4 means the write landed but read back different (firmware clamped or
    // quantised it) which is not a failure.
    match out.status.code() {
        Some(0) | Some(4) => Ok(()),
        _ => {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.trim();
            if err.contains("password is required") {
                Err("helper not authorised - run: sudo ./install.sh".into())
            } else if err.is_empty() {
                Err(format!("write to {path} failed"))
            } else {
                Err(err.lines().next().unwrap_or("write failed").to_string())
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Telemetry {
    pub cpu_temp: i32,
    pub gpu_temp: i32,
    pub fan_rpm: i32,
    pub fan_max: i32,
    pub cpu_mhz: i32,
    pub pl1: i32,
    pub pl2: i32,
    pub tau: i32,
    pub powermode: i32,
    pub prochot: i32,
    pub gpu_watts: f32,
    pub fan_fullspeed: bool,
    pub minifancurve: bool,
}

pub struct Hw {
    pub legion_hwmon: Option<PathBuf>,
    pub coretemp: Option<PathBuf>,
}

impl Hw {
    pub fn new() -> Self {
        Self { legion_hwmon: hwmon("legion_hwmon"), coretemp: hwmon("coretemp") }
    }

    pub fn present(&self) -> bool {
        self.legion_hwmon.is_some()
    }

    fn lh(&self, node: &str) -> Option<String> {
        Some(self.legion_hwmon.as_ref()?.join(node).to_str()?.to_string())
    }

    pub fn fan_max(&self) -> i32 {
        self.lh("fan1_max").and_then(|p| read_i32(&p)).filter(|v| *v > 0).unwrap_or(5400)
    }

    pub fn telemetry(&self) -> Telemetry {
        let mut t = Telemetry::default();
        t.fan_max = self.fan_max();
        if let Some(p) = self.coretemp.as_ref().and_then(|c| c.join("temp1_input").to_str().map(String::from)) {
            t.cpu_temp = read_i32(&p).unwrap_or(0) / 1000;
        }
        if let Some(p) = self.lh("temp2_input") {
            t.gpu_temp = read_i32(&p).unwrap_or(0) / 1000;
        }
        if let Some(p) = self.lh("fan1_input") {
            t.fan_rpm = read_i32(&p).unwrap_or(0);
        }
        if let Some(p) = self.lh("minifancurve") {
            t.minifancurve = read(&p).as_deref() == Some("1");
        }
        t.fan_fullspeed = read(&format!("{LEGION}/fan_fullspeed")).as_deref() == Some("1");
        t.powermode = read_i32(&format!("{LEGION}/powermode")).unwrap_or(-1);
        t.pl1 = read_i32(&format!("{RAPL}/constraint_0_power_limit_uw")).unwrap_or(0) / 1_000_000;
        t.pl2 = read_i32(&format!("{RAPL}/constraint_1_power_limit_uw")).unwrap_or(0) / 1_000_000;
        t.tau = read_i32(&format!("{RAPL}/constraint_0_time_window_us")).unwrap_or(0) / 1_000_000;
        t.prochot =
            read_i32("/sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count").unwrap_or(0);

        let mut sum = 0i64;
        let mut n = 0i64;
        if let Ok(rd) = fs::read_dir("/sys/devices/system/cpu") {
            for e in rd.flatten() {
                let p = e.path().join("cpufreq/scaling_cur_freq");
                if let Some(v) = p.to_str().and_then(read_i32) {
                    sum += v as i64;
                    n += 1;
                }
            }
        }
        t.cpu_mhz = if n > 0 { (sum / n / 1000) as i32 } else { 0 };

        if let Ok(o) = Command::new("nvidia-smi")
            .args(["--query-gpu=power.draw", "--format=csv,noheader,nounits"])
            .output()
        {
            t.gpu_watts = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0.0);
        }
        t
    }

    /// One curve point as the hardware describes it: a trip temperature and the
    /// fan speed requested at that temperature.
    pub fn read_curve(&self) -> Vec<(i32, i32)> {
        let mx = self.fan_max();
        (1..=POINTS)
            .map(|i| {
                let t = self.lh(&format!("pwm1_auto_point{i}_temp")).and_then(|p| read_i32(&p)).unwrap_or(0);
                let pwm = self.lh(&format!("pwm1_auto_point{i}_pwm")).and_then(|p| read_i32(&p)).unwrap_or(0);
                (t, pwm_to_rpm(pwm, mx))
            })
            .collect()
    }

    /// Write a full curve.
    ///
    /// Points go highest-first: the firmware requires trip temperatures to
    /// increase monotonically, and writing low-to-high can transiently invert
    /// a pair and get the write rejected.
    ///
    /// fan_fullspeed pins the fan to maximum and overrides the curve entirely,
    /// and minifancurve permits a complete stop, so both are cleared first or
    /// the curve is cosmetic.
    /// Curve writes are silently rejected outside custom powermode (255): the
    /// write reports success and the value simply does not change. Verified by
    /// writing the same point in mode 3 (ignored) and mode 255 (accepted).
    pub fn write_curve(&self, pts: &[(i32, i32)]) -> Vec<String> {
        let mut errs = Vec::new();
        let mx = self.fan_max();

        if read_i32(&format!("{LEGION}/powermode")) != Some(255) {
            if let Err(e) = write(&format!("{LEGION}/powermode"), 255) {
                errs.push(format!("could not enter custom powermode: {e}"));
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }

        if read(&format!("{LEGION}/fan_fullspeed")).as_deref() != Some("0") {
            let _ = write(&format!("{LEGION}/fan_fullspeed"), 0);
        }
        if let Some(p) = self.lh("minifancurve") {
            if read(&p).as_deref() == Some("1") {
                let _ = write(&p, 0);
            }
        }

        for (idx, (temp, rpm)) in pts.iter().enumerate().rev() {
            let i = idx + 1;
            let hyst = if idx == 0 { (temp - 8).max(0) } else { (temp - 5).max(0) };
            for (node, val) in [
                (format!("pwm1_auto_point{i}_temp"), *temp),
                (format!("pwm1_auto_point{i}_temp_hyst"), hyst),
                (format!("pwm1_auto_point{i}_pwm"), rpm_to_pwm(*rpm, mx)),
            ] {
                if let Some(p) = self.lh(&node) {
                    if let Err(e) = write(&p, val) {
                        errs.push(format!("point {i}: {e}"));
                    }
                }
            }
        }
        errs
    }

    pub fn set_power(&self, pl1: i32, pl2: i32, tau: i32) -> Vec<String> {
        let mut errs = Vec::new();
        for (node, val) in [
            (format!("{RAPL}/constraint_0_power_limit_uw"), pl1 * 1_000_000),
            (format!("{RAPL}/constraint_1_power_limit_uw"), pl2 * 1_000_000),
            (format!("{RAPL}/constraint_0_time_window_us"), tau * 1_000_000),
        ] {
            if let Err(e) = write(&node, val) {
                errs.push(e);
            }
        }
        errs
    }

    pub fn set_gpu(&self, ctgp: i32, ppab: i32) -> Vec<String> {
        let mut errs = Vec::new();
        for (node, val) in [
            (format!("{LEGION}/gpu_ctgp_powerlimit"), ctgp),
            (format!("{LEGION}/gpu_ppab_powerlimit"), ppab),
        ] {
            if let Err(e) = write(&node, val) {
                errs.push(e);
            }
        }
        errs
    }

    pub fn gpu_limits(&self) -> (i32, i32) {
        (
            read_i32(&format!("{LEGION}/gpu_ctgp_powerlimit")).unwrap_or(0),
            read_i32(&format!("{LEGION}/gpu_ppab_powerlimit")).unwrap_or(0),
        )
    }

    pub fn set_powermode(&self, mode: i32) -> Result<(), String> {
        write(&format!("{LEGION}/powermode"), mode)
    }

    /// Reload the firmware curve by bouncing the power mode; the EC repopulates
    /// its table on every mode change.
    pub fn restore_stock(&self) -> Result<(), String> {
        let cur = read_i32(&format!("{LEGION}/powermode")).unwrap_or(255);
        let other = if cur == 3 { 2 } else { 3 };
        write(&format!("{LEGION}/powermode"), other)?;
        std::thread::sleep(std::time::Duration::from_millis(1800));
        write(&format!("{LEGION}/powermode"), cur)
    }
}

pub fn pwm_to_rpm(pwm: i32, max_rpm: i32) -> i32 {
    ((pwm as f32 * max_rpm as f32 / 255.0) / 100.0).round() as i32 * 100
}

pub fn rpm_to_pwm(rpm: i32, max_rpm: i32) -> i32 {
    ((rpm as f32 * 255.0 / max_rpm as f32).round() as i32).clamp(0, 255)
}
