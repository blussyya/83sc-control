use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LEGION: &str = "/sys/bus/platform/devices/PNP0C09:00";
pub const RAPL: &str = "/sys/class/powercap/intel-rapl:0";
pub const HELPER: &str = "/usr/local/lib/83sc-control/helper.py";
pub const PSTATE: &str = "/sys/devices/system/cpu/intel_pstate";

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
    pub undervolt_mv: i32,
    pub max_perf_pct: i32,
    pub on_battery: bool,
}

/// Everything outside the thermal/power core: battery, input, panel and the
/// embedded controller's own limits. Polled less often than Telemetry because
/// none of it moves on its own.
#[derive(Clone, Debug, Default)]
pub struct Extras {
    pub bat_capacity: i32,
    pub bat_cycles: i32,
    pub bat_health: i32,
    pub bat_volts: f32,
    pub bat_watts: f32,
    pub bat_status: String,
    pub bat_model: String,
    pub conservation: bool,
    pub rapid_charge: bool,

    pub kbd_backlight: i32,
    pub kbd_max: i32,
    pub fn_lock: bool,
    pub winkey: bool,
    pub touchpad: bool,
    pub flip_to_start: bool,

    pub overdrive: bool,
    pub refresh_hz: i32,

    pub cpu_temp_limit: i32,
    pub gpu_temp_limit: i32,
    pub cross_loading: i32,
    pub ec_tau: i32,
    pub pl_coupling: bool,
    pub gpu_boost: i32,
    pub gpu_target_offset: i32,

    pub gpu_name: String,
    pub gpu_pl_max: i32,
    pub igpu_mode: i32,
}

pub const KBD_LEDS: [&str; 2] = [
    "/sys/class/leds/platform::kbd_backlight/brightness",
    "/sys/class/leds/platform::kbd_backlight_1/brightness",
];

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
        t.max_perf_pct = read_i32(&format!("{PSTATE}/max_perf_pct")).unwrap_or(100);
        t.on_battery = read("/sys/class/power_supply/ACAD/online").as_deref() == Some("0");
        t.undervolt_mv = read_undervolt();

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

    pub fn extras(&self) -> Extras {
        let mut e = Extras::default();
        let bat = "/sys/class/power_supply/BAT1";
        e.bat_capacity = read_i32(&format!("{bat}/capacity")).unwrap_or(0);
        e.bat_cycles = read_i32(&format!("{bat}/cycle_count")).unwrap_or(0);
        let full = read_i32(&format!("{bat}/energy_full")).unwrap_or(0);
        let design = read_i32(&format!("{bat}/energy_full_design")).unwrap_or(0);
        e.bat_health = if design > 0 { (full as i64 * 100 / design as i64) as i32 } else { 0 };
        e.bat_volts = read_i32(&format!("{bat}/voltage_now")).unwrap_or(0) as f32 / 1_000_000.0;
        e.bat_watts = read_i32(&format!("{bat}/power_now")).unwrap_or(0) as f32 / 1_000_000.0;
        e.bat_status = read(&format!("{bat}/status")).unwrap_or_default();
        e.bat_model = read(&format!("{bat}/model_name")).unwrap_or_default();

        let flag = |n: &str| read(&format!("{LEGION}/{n}")).as_deref() == Some("1");
        e.conservation = flag("battery_conservation");
        e.rapid_charge = flag("rapidcharge");
        e.fn_lock = flag("fn_lock");
        e.winkey = flag("winkey");
        e.touchpad = flag("touchpad");
        e.flip_to_start = flag("flip_to_start");
        e.overdrive = flag("overdrive");
        e.pl_coupling = flag("cpu_pl_coupling");

        e.kbd_backlight = read_i32(KBD_LEDS[0]).unwrap_or(0);
        e.kbd_max = read_i32("/sys/class/leds/platform::kbd_backlight/max_brightness").unwrap_or(2);

        let ec = |n: &str| read_i32(&format!("{LEGION}/{n}")).unwrap_or(0);
        e.cpu_temp_limit = ec("cpu_temperature_limit");
        e.gpu_temp_limit = ec("gpu_temperature_limit");
        e.cross_loading = ec("cpu_cross_loading_powerlimit");
        e.ec_tau = ec("cpu_l1_tau");
        // On this model gpu_oc goes through the WMI3 clamped path, where it is
        // GPU power boost in watts rather than the on/off switch the name
        // suggests.
        e.gpu_boost = ec("gpu_oc");
        e.gpu_target_offset = ec("gpu_power_target_offset");
        e.igpu_mode = ec("igpumode");

        e.refresh_hz = current_refresh().unwrap_or(0);

        if let Ok(o) = Command::new("nvidia-smi")
            .args(["--query-gpu=name,power.max_limit", "--format=csv,noheader,nounits"])
            .output()
        {
            let s = String::from_utf8_lossy(&o.stdout);
            let mut f = s.trim().split(',');
            e.gpu_name = f.next().unwrap_or("").trim().to_string();
            e.gpu_pl_max = f.next().unwrap_or("0").trim().parse::<f32>().unwrap_or(0.0) as i32;
        }
        e
    }

    /// One physical zone behind two LED nodes: ideapad and legion_laptop each
    /// register one and they drive the same EC register, so both are written to
    /// keep the readback consistent whichever one is read.
    pub fn set_kbd_backlight(&self, level: i32) -> Result<(), String> {
        let level = level.clamp(0, 2);
        let mut first = Ok(());
        for node in KBD_LEDS {
            let r = write(node, level);
            if first.is_ok() {
                first = r;
            }
        }
        first
    }

    /// Conservation mode and rapid charge are mutually exclusive in firmware,
    /// so enabling one clears the other rather than letting the EC arbitrate.
    pub fn set_charge_mode(&self, conservation: bool, rapid: bool) -> Vec<String> {
        let mut errs = Vec::new();
        if conservation && rapid {
            return vec!["conservation mode and rapid charge cannot both be on".into()];
        }
        for (node, on) in [("battery_conservation", conservation), ("rapidcharge", rapid)] {
            if let Err(e) = write(&format!("{LEGION}/{node}"), if on { 1 } else { 0 }) {
                errs.push(e);
            }
        }
        errs
    }

    pub fn set_flag(&self, node: &str, on: bool) -> Result<(), String> {
        write(&format!("{LEGION}/{node}"), if on { 1 } else { 0 })
    }

    pub fn set_ec(&self, node: &str, value: i32) -> Result<(), String> {
        write(&format!("{LEGION}/{node}"), value)
    }

    /// Undervolt goes through the helper's dedicated verb rather than a path
    /// write: it edits /etc/intel-undervolt.conf and invokes the tool, and that
    /// file is outside the writable sysfs prefixes by design.
    pub fn set_undervolt(&self, mv: i32) -> Result<(), String> {
        let out = Command::new("sudo")
            .args(["-n", HELPER, "undervolt", &mv.to_string()])
            .output()
            .map_err(|e| format!("could not run helper: {e}"))?;
        match out.status.code() {
            Some(0) => Ok(()),
            // rc 4 = applied value did not match request; the BIOS UnderVolt
            // Protection gate silently swallows writes when enabled.
            Some(4) => Err("offset did not take - check UnderVolt Protection in BIOS".into()),
            _ => Err(String::from_utf8_lossy(&out.stderr).trim().lines().next()
                     .unwrap_or("undervolt failed").to_string()),
        }
    }

    /// Make the undervolt survive reboot via intel-undervolt's systemd unit.
    pub fn set_undervolt_persist(&self, on: bool) -> Result<(), String> {
        let out = Command::new("sudo")
            .args(["-n", HELPER, "undervolt-persist", if on { "1" } else { "0" }])
            .output()
            .map_err(|e| format!("could not run helper: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().lines().next()
                .unwrap_or("could not change boot persistence").to_string())
        }
    }

    /// Percentage of maximum CPU performance. This is the direct Linux
    /// equivalent of the Windows power-plan "maximum processor state" percent.
    pub fn set_max_perf(&self, pct: i32) -> Result<(), String> {
        write(&format!("{PSTATE}/max_perf_pct"), pct.clamp(10, 100))
    }

    pub fn set_fan_fullspeed(&self, on: bool) -> Result<(), String> {
        write(&format!("{LEGION}/fan_fullspeed"), if on { 1 } else { 0 })
    }

    /// Snapshot whatever the hardware is currently doing into
    /// /etc/83sc-control/boot.conf, which 83sc-thermal.service replays at boot.
    pub fn boot_save(&self) -> Result<(), String> {
        let out = Command::new("sudo").args(["-n", HELPER, "boot-save"]).output()
            .map_err(|e| format!("could not run helper: {e}"))?;
        if out.status.success() { Ok(()) } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().lines().next()
                .unwrap_or("boot-save failed").to_string())
        }
    }

    pub fn power_plan(&self) -> Option<String> {
        let o = Command::new("powerprofilesctl").arg("get").output().ok()?;
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if s.is_empty() { None } else { Some(s) }
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

/// Current core undervolt offset in mV, as a positive magnitude (0 = none).
pub fn read_undervolt() -> i32 {
    let out = match Command::new("intel-undervolt").arg("read").output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return 0,
    };
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("CPU (0): ") {
            let v: f32 = rest.trim_end_matches(" mV").trim().parse().unwrap_or(0.0);
            // keep the sign: negative is undervolt, positive is overvolt
            return v.round() as i32;
        }
    }
    0
}

/// The refresh rate is a compositor setting rather than a firmware one, so it
/// goes through kscreen-doctor in the user's own session and never touches the
/// helper.
fn kscreen() -> Option<serde_json::Value> {
    let o = Command::new("kscreen-doctor").arg("-j").output().ok()?;
    serde_json::from_slice(&o.stdout).ok()
}

fn panel(v: &serde_json::Value) -> Option<&serde_json::Value> {
    v["outputs"].as_array()?.iter().find(|o| o["enabled"] == true)
}

pub fn current_refresh() -> Option<i32> {
    let v = kscreen()?;
    let out = panel(&v)?;
    let cur = out["currentModeId"].as_str()?;
    out["modes"]
        .as_array()?
        .iter()
        .find(|m| m["id"].as_str() == Some(cur))
        .and_then(|m| m["refreshRate"].as_f64())
        .map(|r| r.round() as i32)
}

/// Rates offered at the resolution the panel is running now, highest first.
/// Switching resolution as a side effect of changing refresh rate would be a
/// surprise, so lower-resolution modes are filtered out.
pub fn refresh_rates() -> Vec<i32> {
    let mut out = Vec::new();
    if let Some(v) = kscreen() {
        if let Some(o) = panel(&v) {
            let cur = o["currentModeId"].as_str().unwrap_or("");
            let modes = match o["modes"].as_array() {
                Some(m) => m,
                None => return out,
            };
            let (w, h) = modes
                .iter()
                .find(|m| m["id"].as_str() == Some(cur))
                .map(|m| (m["size"]["width"].as_i64(), m["size"]["height"].as_i64()))
                .unwrap_or((None, None));
            for m in modes {
                if m["size"]["width"].as_i64() != w || m["size"]["height"].as_i64() != h {
                    continue;
                }
                if let Some(r) = m["refreshRate"].as_f64() {
                    let hz = r.round() as i32;
                    if !out.contains(&hz) {
                        out.push(hz);
                    }
                }
            }
        }
    }
    out.sort_unstable_by(|a, b| b.cmp(a));
    out
}

pub fn set_refresh(hz: i32) -> Result<(), String> {
    let v = kscreen().ok_or("kscreen-doctor is not available")?;
    let out = panel(&v).ok_or("no enabled display found")?;
    let name = out["name"].as_str().ok_or("display has no name")?;
    let cur = out["currentModeId"].as_str().unwrap_or("");
    let modes = out["modes"].as_array().ok_or("display reports no modes")?;
    let (w, h) = modes
        .iter()
        .find(|m| m["id"].as_str() == Some(cur))
        .map(|m| (m["size"]["width"].as_i64(), m["size"]["height"].as_i64()))
        .unwrap_or((None, None));
    let id = modes
        .iter()
        .find(|m| {
            m["size"]["width"].as_i64() == w
                && m["size"]["height"].as_i64() == h
                && m["refreshRate"].as_f64().map(|r| r.round() as i32) == Some(hz)
        })
        .and_then(|m| m["id"].as_str())
        .ok_or_else(|| format!("the panel has no {hz} Hz mode at this resolution"))?;

    let o = Command::new("kscreen-doctor")
        .arg(format!("output.{name}.mode.{id}"))
        .output()
        .map_err(|e| format!("could not run kscreen-doctor: {e}"))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().lines().last()
            .unwrap_or("refresh rate change failed").to_string())
    }
}

/// Processes currently holding the discrete GPU awake, as "name (MiB)".
pub fn gpu_clients() -> Vec<String> {
    let o = match Command::new("nvidia-smi")
        .args(["--query-compute-apps=process_name,used_memory", "--format=csv,noheader,nounits"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let mut v: Vec<String> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut f = l.split(',');
            let name = f.next().unwrap_or("").trim();
            let name = name.rsplit('/').next().unwrap_or(name);
            let mem = f.next().unwrap_or("").trim();
            format!("{name} ({mem} MiB)")
        })
        .collect();
    // Graphics clients do not show up in the compute-apps query, so fall back to
    // the runtime power state to say whether anything is holding the card up.
    if v.is_empty() && read("/sys/bus/pci/devices/0000:01:00.0/power/runtime_status").as_deref() == Some("active") {
        v.push("awake, no compute clients".into());
    }
    v
}

pub fn pwm_to_rpm(pwm: i32, max_rpm: i32) -> i32 {
    ((pwm as f32 * max_rpm as f32 / 255.0) / 100.0).round() as i32 * 100
}

pub fn rpm_to_pwm(rpm: i32, max_rpm: i32) -> i32 {
    ((rpm as f32 * 255.0 / max_rpm as f32).round() as i32).clamp(0, 255)
}
