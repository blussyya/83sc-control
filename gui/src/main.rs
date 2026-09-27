mod hw;
mod kbd_idle;
mod profiles;

use hw::Hw;
use profiles::{Config, Profile};
use slint::{ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

slint::include_modules!();

struct App {
    hw: Hw,
    cfg: RefCell<Config>,
    /// Curve currently being edited. Kept separate from what the hardware
    /// reports so a half-finished edit is not overwritten by the poll timer.
    draft: RefCell<Vec<(i32, i32)>>,
    dirty: RefCell<bool>,
    /// Refresh rates the panel offers, in the order the combo box shows them.
    rates: RefCell<Vec<i32>>,
}

fn to_extras(e: &hw::Extras) -> Extras {
    Extras {
        bat_capacity: e.bat_capacity,
        bat_cycles: e.bat_cycles,
        bat_health: e.bat_health,
        bat_volts: e.bat_volts,
        bat_watts: e.bat_watts,
        bat_status: e.bat_status.clone().into(),
        bat_model: e.bat_model.clone().into(),
        kbd_max: e.kbd_max,
        refresh_hz: e.refresh_hz,
        gpu_name: e.gpu_name.clone().into(),
        gpu_pl_max: e.gpu_pl_max,
        igpu_mode: e.igpu_mode,
    }
}

/// Push hardware state into the editable widgets. Only called at startup and
/// after a write lands -- the poll timer must not do it, or a checkbox would
/// snap back under the user's finger while a slow EC write is still in flight.
fn seed_extras(ui: &MainWindow, e: &hw::Extras) {
    ui.set_tog_conservation(e.conservation);
    ui.set_tog_rapid(e.rapid_charge);
    ui.set_tog_fnlock(e.fn_lock);
    ui.set_tog_winkey(e.winkey);
    ui.set_tog_touchpad(e.touchpad);
    ui.set_tog_flip(e.flip_to_start);
    ui.set_tog_overdrive(e.overdrive);
    ui.set_tog_plcoupling(e.pl_coupling);
    ui.set_edit_kbd(e.kbd_backlight);
    ui.set_tog_kbd_idle(kbd_idle::enabled());
    ui.set_edit_kbd_idle_secs(kbd_idle::timeout_secs());
    ui.set_edit_cpu_temp(e.cpu_temp_limit);
    ui.set_edit_gpu_temp(e.gpu_temp_limit);
    ui.set_edit_crossload(e.cross_loading);
    ui.set_edit_ec_tau(e.ec_tau);
    ui.set_edit_gpu_boost(e.gpu_boost);
    ui.set_edit_gpu_offset(e.gpu_target_offset);
}

fn to_curve_model(pts: &[(i32, i32)]) -> ModelRc<CurvePoint> {
    ModelRc::new(VecModel::from(
        pts.iter().map(|(t, r)| CurvePoint { temp: *t, rpm: *r }).collect::<Vec<_>>(),
    ))
}

/// Profile fields a profile may decline to have an opinion about. 0 -- or -1
/// for the backlight -- means leave whatever the machine is already doing, so a
/// purely thermal profile does not drag the keyboard light around with it.
fn apply_profile_extras(hw: &Hw, p: &Profile) -> Vec<String> {
    let mut errs = Vec::new();
    for (node, val) in [
        ("cpu_temperature_limit", p.cpu_temp_limit),
        ("gpu_temperature_limit", p.gpu_temp_limit),
        ("gpu_oc", p.gpu_boost),
        ("gpu_power_target_offset", p.gpu_target_offset),
    ] {
        if val > 0 {
            if let Err(e) = hw.set_ec(node, val) {
                errs.push(e);
            }
        }
    }
    if p.kbd_backlight >= 0 {
        if let Err(e) = hw.set_kbd_backlight(p.kbd_backlight) {
            errs.push(e);
        }
    }
    if p.refresh_hz > 0 {
        if let Err(e) = hw::set_refresh(p.refresh_hz) {
            errs.push(e);
        }
    }
    errs
}

fn report(ui: &MainWindow, errs: &[String], ok: &str) {
    if errs.is_empty() {
        ui.set_status_error(false);
        ui.set_status(ok.into());
    } else {
        ui.set_status_error(true);
        ui.set_status(errs.join("; ").into());
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    let app = Rc::new(App {
        hw: Hw::new(),
        cfg: RefCell::new(profiles::load()),
        draft: RefCell::new(Vec::new()),
        dirty: RefCell::new(false),
        rates: RefCell::new(hw::refresh_rates()),
    });

    ui.set_hw_ok(app.hw.present());
    if !app.hw.present() {
        ui.set_status("legion_laptop is not loaded".into());
        ui.set_status_error(true);
    }

    // seed editors from live hardware
    let t0 = app.hw.telemetry();
    ui.set_edit_pl1(if t0.pl1 > 0 { t0.pl1 } else { 45 });
    ui.set_edit_pl2(if t0.pl2 > 0 { t0.pl2 } else { 90 });
    ui.set_edit_tau(if t0.tau > 0 { t0.tau } else { 8 });
    let (ctgp, ppab) = app.hw.gpu_limits();
    ui.set_edit_ctgp(ctgp);
    ui.set_edit_ppab(ppab);
    ui.set_edit_uv(t0.undervolt_mv);
    ui.set_edit_maxperf(if t0.max_perf_pct > 0 { t0.max_perf_pct } else { 100 });
    ui.set_persist(app.cfg.borrow().persist);

    *app.draft.borrow_mut() = app.hw.read_curve();
    ui.set_curve(to_curve_model(&app.draft.borrow()));

    let e0 = app.hw.extras();
    ui.set_extras(to_extras(&e0));
    seed_extras(&ui, &e0);
    {
        let rates = app.rates.borrow();
        let labels: Vec<SharedString> =
            rates.iter().map(|h| SharedString::from(format!("{h} Hz"))).collect();
        ui.set_refresh_options(ModelRc::new(VecModel::from(labels)));
        ui.set_edit_refresh_idx(rates.iter().position(|h| *h == e0.refresh_hz).unwrap_or(0) as i32);
    }
    ui.set_gpu_clients(ModelRc::new(VecModel::from(
        hw::gpu_clients().into_iter().map(SharedString::from).collect::<Vec<_>>(),
    )));

    let refresh_profiles = {
        let app = app.clone();
        move |ui: &MainWindow| {
            let names: Vec<SharedString> = app
                .cfg
                .borrow()
                .profiles
                .iter()
                .map(|p| {
                    let bound = app
                        .cfg
                        .borrow()
                        .bindings
                        .iter()
                        .find(|(_, n)| *n == p.name)
                        .map(|(m, _)| format!("  [{}]", profiles::mode_name(*m)))
                        .unwrap_or_default();
                    let cfg = app.cfg.borrow();
                    let src = if cfg.on_battery_profile.as_deref() == Some(p.name.as_str()) {
                        "  [batt]".to_string()
                    } else if cfg.on_ac_profile.as_deref() == Some(p.name.as_str()) {
                        "  [AC]".to_string()
                    } else if let Some(pl) = &p.power_plan {
                        format!("  [{pl}]")
                    } else {
                        String::new()
                    };
                    SharedString::from(format!("{}{}{}", p.name, bound, src))
                })
                .collect();
            ui.set_profile_names(ModelRc::new(VecModel::from(names)));
        }
    };
    refresh_profiles(&ui);

    // ---- callbacks ----

    ui.on_point_changed({
        let app = app.clone();
        let w = ui.as_weak();
        move |idx, temp, rpm| {
            let ui = w.unwrap();
            let mut d = app.draft.borrow_mut();
            let i = idx as usize;
            if i < d.len() {
                let mx = app.hw.fan_max();
                d[i] = (temp.clamp(0, 120), rpm.clamp(0, mx));
                *app.dirty.borrow_mut() = true;
                ui.set_status("edited — press Apply fan curve".into());
                ui.set_status_error(false);
            }
        }
    });

    ui.on_apply_curve({
        let app = app.clone();
        let w = ui.as_weak();
        move || {
            let ui = w.unwrap();
            let pts = app.draft.borrow().clone();
            // The firmware rejects a non-monotonic table outright, so catch it
            // here and say which point is wrong rather than failing opaquely.
            for i in 1..pts.len() {
                if pts[i].0 < pts[i - 1].0 {
                    ui.set_status_error(true);
                    ui.set_status(
                        format!(
                            "point {} temperature ({}C) is below point {} ({}C) — must increase",
                            i + 1,
                            pts[i].0,
                            i,
                            pts[i - 1].0
                        )
                        .into(),
                    );
                    return;
                }
            }
            let errs = app.hw.write_curve(&pts);
            *app.dirty.borrow_mut() = false;
            let live = app.hw.read_curve();
            *app.draft.borrow_mut() = live.clone();
            ui.set_curve(to_curve_model(&live));
            report(&ui, &errs, "fan curve applied");
        }
    });

    ui.on_apply_power({
        let app = app.clone();
        let w = ui.as_weak();
        move || {
            let ui = w.unwrap();
            let errs = app.hw.set_power(ui.get_edit_pl1(), ui.get_edit_pl2(), ui.get_edit_tau());
            report(&ui, &errs, "power limits applied");
        }
    });

    ui.on_apply_gpu({
        let app = app.clone();
        let w = ui.as_weak();
        move || {
            let ui = w.unwrap();
            let errs = app.hw.set_gpu(ui.get_edit_ctgp(), ui.get_edit_ppab());
            let (c, p) = app.hw.gpu_limits();
            ui.set_edit_ctgp(c);
            ui.set_edit_ppab(p);
            report(&ui, &errs, &format!("GPU limits applied (firmware set {c} + {p} W)"));
        }
    });

    ui.on_apply_undervolt({
        let app = app.clone();
        let w = ui.as_weak();
        move || {
            let ui = w.unwrap();
            let mv = ui.get_edit_uv();
            match app.hw.set_undervolt(mv) {
                Ok(()) => {
                    ui.set_status_error(false);
                    ui.set_status(match mv {
                        0 => SharedString::from("voltage offset cleared"),
                        m if m < 0 => format!("{m} mV applied - not persistent until 'Re-apply at boot'").into(),
                        m => format!("+{m} mV OVERVOLT applied - raises heat and wear").into(),
                    });
                }
                Err(e) => { ui.set_status_error(true); ui.set_status(e.into()); }
            }
        }
    });

    ui.on_apply_maxperf({
        let app = app.clone();
        let w = ui.as_weak();
        move || {
            let ui = w.unwrap();
            let pct = ui.get_edit_maxperf();
            match app.hw.set_max_perf(pct) {
                Ok(()) => {
                    ui.set_status_error(false);
                    ui.set_status(format!("CPU ceiling set to {pct}%").into());
                }
                Err(e) => { ui.set_status_error(true); ui.set_status(e.into()); }
            }
        }
    });

    ui.on_set_flag({
        let app = app.clone();
        let w = ui.as_weak();
        move |node, on| {
            let ui = w.unwrap();
            let node = node.to_string();
            match app.hw.set_flag(&node, on) {
                Ok(()) => {
                    // Re-seed from hardware rather than trusting the click: a
                    // firmware that swallowed the write would otherwise leave the
                    // checkbox showing a state the machine is not in.
                    let e = app.hw.extras();
                    seed_extras(&ui, &e);
                    ui.set_extras(to_extras(&e));
                    ui.set_status_error(false);
                    ui.set_status(format!("{node} {}", if on { "on" } else { "off" }).into());
                }
                Err(e) => {
                    seed_extras(&ui, &app.hw.extras());
                    ui.set_status_error(true);
                    ui.set_status(e.into());
                }
            }
        }
    });

    ui.on_set_charge_mode({
        let app = app.clone();
        let w = ui.as_weak();
        move |conservation, rapid| {
            let ui = w.unwrap();
            let errs = app.hw.set_charge_mode(conservation, rapid);
            let e = app.hw.extras();
            seed_extras(&ui, &e);
            ui.set_extras(to_extras(&e));
            report(
                &ui,
                &errs,
                match (e.conservation, e.rapid_charge) {
                    (true, _) => "conservation mode on — charging stops near 60%",
                    (_, true) => "rapid charge on",
                    _ => "normal charging",
                },
            );
        }
    });

    // Slider drags fire per pixel; restarting a systemd unit that often is
    // pointless churn, so settle for 400ms before touching it.
    ui.on_apply_kbd_idle({
        let w = ui.as_weak();
        let timer = slint::Timer::default();
        move |on, secs| {
            let w = w.clone();
            timer.start(slint::TimerMode::SingleShot, std::time::Duration::from_millis(400), move || {
                let ui = w.unwrap();
                match kbd_idle::apply(on, secs) {
                    Ok(()) => {
                        ui.set_status_error(false);
                        ui.set_status(if on {
                            format!("keyboard backlight off after {secs}s idle").into()
                        } else {
                            SharedString::from("idle backlight off disabled")
                        });
                    }
                    Err(e) => { ui.set_status_error(true); ui.set_status(e.into()); }
                }
            });
        }
    });

    ui.on_apply_kbd({
        let app = app.clone();
        let w = ui.as_weak();
        move |level| {
            let ui = w.unwrap();
            match app.hw.set_kbd_backlight(level) {
                Ok(()) => {
                    ui.set_status_error(false);
                    ui.set_status(match level {
                        0 => SharedString::from("keyboard backlight off"),
                        n => format!("keyboard backlight level {n}").into(),
                    });
                }
                Err(e) => { ui.set_status_error(true); ui.set_status(e.into()); }
            }
        }
    });

    ui.on_apply_refresh({
        let app = app.clone();
        let w = ui.as_weak();
        move |idx| {
            let ui = w.unwrap();
            let hz = match app.rates.borrow().get(idx as usize) {
                Some(h) => *h,
                None => return,
            };
            match hw::set_refresh(hz) {
                Ok(()) => {
                    ui.set_extras(to_extras(&app.hw.extras()));
                    ui.set_status_error(false);
                    ui.set_status(format!("panel now at {hz} Hz").into());
                }
                Err(e) => { ui.set_status_error(true); ui.set_status(e.into()); }
            }
        }
    });

    ui.on_apply_ec_limits({
        let app = app.clone();
        let w = ui.as_weak();
        move || {
            let ui = w.unwrap();
            let mut errs = Vec::new();
            for (node, val) in [
                ("cpu_temperature_limit", ui.get_edit_cpu_temp()),
                ("cpu_cross_loading_powerlimit", ui.get_edit_crossload()),
                ("cpu_l1_tau", ui.get_edit_ec_tau()),
            ] {
                if let Err(e) = app.hw.set_ec(node, val) {
                    errs.push(e);
                }
            }
            let e = app.hw.extras();
            seed_extras(&ui, &e);
            report(
                &ui,
                &errs,
                &format!(
                    "EC limits applied — throttle {} C, cross-load {} W, window {} s",
                    e.cpu_temp_limit, e.cross_loading, e.ec_tau
                ),
            );
        }
    });

    ui.on_apply_gpu_extra({
        let app = app.clone();
        let w = ui.as_weak();
        move || {
            let ui = w.unwrap();
            let mut errs = Vec::new();
            for (node, val) in [
                ("gpu_temperature_limit", ui.get_edit_gpu_temp()),
                ("gpu_oc", ui.get_edit_gpu_boost()),
                ("gpu_power_target_offset", ui.get_edit_gpu_offset()),
            ] {
                if let Err(e) = app.hw.set_ec(node, val) {
                    errs.push(e);
                }
            }
            let e = app.hw.extras();
            seed_extras(&ui, &e);
            report(
                &ui,
                &errs,
                &format!(
                    "GPU firmware limits applied — throttle {} C, boost {} W, offset {} W",
                    e.gpu_temp_limit, e.gpu_boost, e.gpu_target_offset
                ),
            );
        }
    });

    ui.on_set_persist({
        let app = app.clone();
        let w = ui.as_weak();
        move |on| {
            let ui = w.unwrap();
            app.cfg.borrow_mut().persist = on;
            let _ = profiles::save(&app.cfg.borrow());
            // Power limits and the fan curve already persist via
            // 83sc-thermal.service; the undervolt needs its own unit enabled.
            let mut errs = Vec::new();
            if let Err(e) = app.hw.set_undervolt_persist(on) { errs.push(e); }
            if on {
                // snapshot everything live so the boot unit replays the full state,
                // not just power limits
                if let Err(e) = app.hw.boot_save() { errs.push(e); }
            }
            if errs.is_empty() {
                ui.set_status_error(false);
                ui.set_status(if on {
                    SharedString::from("saved - full state will be restored at boot")
                } else {
                    SharedString::from("boot restore disabled")
                });
            } else {
                ui.set_status_error(true);
                ui.set_status(errs.join("; ").into());
            }
        }
    });

    ui.on_bind_power_source({
        let app = app.clone();
        let w = ui.as_weak();
        let refresh = refresh_profiles.clone();
        move |idx, which| {
            let ui = w.unwrap();
            let name = match app.cfg.borrow().profiles.get(idx as usize) {
                Some(p) => p.name.clone(),
                None => return,
            };
            {
                let mut cfg = app.cfg.borrow_mut();
                // a profile can only be bound to one source at a time
                if cfg.on_battery_profile.as_deref() == Some(name.as_str()) {
                    cfg.on_battery_profile = None;
                }
                if cfg.on_ac_profile.as_deref() == Some(name.as_str()) {
                    cfg.on_ac_profile = None;
                }
                if let Some(p) = cfg.profiles.iter_mut().find(|p| p.name == name) {
                    p.power_plan = None;
                }
                match which {
                    1 => cfg.on_battery_profile = Some(name.clone()),
                    2 => cfg.on_ac_profile = Some(name.clone()),
                    3..=5 => {
                        let plan = ["power-saver", "balanced", "performance"][(which - 3) as usize];
                        if let Some(p) = cfg.profiles.iter_mut().find(|p| p.name == name) {
                            p.power_plan = Some(plan.to_string());
                        }
                    }
                    _ => {}
                }
                let _ = profiles::save(&cfg);
            }
            refresh(&ui);
            ui.set_status_error(false);
            ui.set_status(match which {
                1 => format!("'{name}' will apply on battery").into(),
                2 => format!("'{name}' will apply on AC").into(),
                3..=5 => {
                    let plan = ["power-saver", "balanced", "performance"][(which - 3) as usize];
                    format!("'{name}' will apply on KDE '{plan}'").into()
                }
                _ => SharedString::from(format!("binding cleared for '{name}'")),
            });
        }
    });

    ui.on_set_powermode({
        let app = app.clone();
        let w = ui.as_weak();
        move |mode| {
            let ui = w.unwrap();
            match app.hw.set_powermode(mode) {
                Ok(()) => {
                    // The EC repopulates its fan table on every mode change.
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    let live = app.hw.read_curve();
                    *app.draft.borrow_mut() = live.clone();
                    ui.set_curve(to_curve_model(&live));
                    ui.set_status_error(false);
                    ui.set_status(format!("power mode: {}", profiles::mode_name(mode)).into());
                }
                Err(e) => {
                    ui.set_status_error(true);
                    ui.set_status(e.into());
                }
            }
        }
    });

    ui.on_restore_stock({
        let app = app.clone();
        let w = ui.as_weak();
        move || {
            let ui = w.unwrap();
            ui.set_status("reloading firmware curve…".into());
            match app.hw.restore_stock() {
                Ok(()) => {
                    let live = app.hw.read_curve();
                    *app.draft.borrow_mut() = live.clone();
                    ui.set_curve(to_curve_model(&live));
                    *app.dirty.borrow_mut() = false;
                    ui.set_status_error(false);
                    ui.set_status("firmware curve restored".into());
                }
                Err(e) => {
                    ui.set_status_error(true);
                    ui.set_status(e.into());
                }
            }
        }
    });

    ui.on_load_profile({
        let app = app.clone();
        let w = ui.as_weak();
        move |idx| {
            let ui = w.unwrap();
            let cfg = app.cfg.borrow();
            if let Some(p) = cfg.profiles.get(idx as usize) {
                *app.draft.borrow_mut() = p.curve.clone();
                ui.set_curve(to_curve_model(&p.curve));
                ui.set_edit_pl1(p.pl1);
                ui.set_edit_pl2(p.pl2);
                ui.set_edit_tau(p.tau);
                ui.set_edit_uv(p.undervolt_mv);
                ui.set_edit_maxperf(p.max_perf_pct);
                if p.cpu_temp_limit > 0 { ui.set_edit_cpu_temp(p.cpu_temp_limit); }
                if p.gpu_temp_limit > 0 { ui.set_edit_gpu_temp(p.gpu_temp_limit); }
                if p.gpu_boost > 0 { ui.set_edit_gpu_boost(p.gpu_boost); }
                if p.gpu_target_offset > 0 { ui.set_edit_gpu_offset(p.gpu_target_offset); }
                if p.kbd_backlight >= 0 { ui.set_edit_kbd(p.kbd_backlight); }
                *app.dirty.borrow_mut() = true;
                ui.set_status_error(false);
                ui.set_status(format!("loaded '{}' — not applied yet", p.name).into());
            }
        }
    });

    ui.on_apply_profile({
        let app = app.clone();
        let w = ui.as_weak();
        move |idx| {
            let ui = w.unwrap();
            let p = match app.cfg.borrow().profiles.get(idx as usize) {
                Some(p) => p.clone(),
                None => return,
            };
            let mut errs = Vec::new();
            // Mode first: it resets the EC fan table, so applying the curve
            // before it would be immediately overwritten.
            if p.powermode != 0 {
                if let Err(e) = app.hw.set_powermode(p.powermode) {
                    errs.push(e);
                }
                std::thread::sleep(std::time::Duration::from_millis(600));
            }
            errs.extend(app.hw.set_power(p.pl1, p.pl2, p.tau));
            if let Err(e) = app.hw.set_undervolt(p.undervolt_mv) {
                errs.push(e);
            }
            if let Err(e) = app.hw.set_max_perf(p.max_perf_pct) {
                errs.push(e);
            }
            if p.gpu_ctgp > 0 || p.gpu_ppab > 0 {
                errs.extend(app.hw.set_gpu(p.gpu_ctgp, p.gpu_ppab));
            }
            errs.extend(apply_profile_extras(&app.hw, &p));
            errs.extend(app.hw.write_curve(&p.curve));
            // after the curve, because write_curve clears it to apply the table
            if p.fan_fullspeed {
                if let Err(e) = app.hw.set_fan_fullspeed(true) { errs.push(e); }
            }

            let e = app.hw.extras();
            seed_extras(&ui, &e);
            ui.set_extras(to_extras(&e));
            let live = app.hw.read_curve();
            *app.draft.borrow_mut() = live.clone();
            ui.set_curve(to_curve_model(&live));
            ui.set_edit_pl1(p.pl1);
            ui.set_edit_pl2(p.pl2);
            ui.set_edit_tau(p.tau);
            ui.set_edit_uv(p.undervolt_mv);
            ui.set_edit_maxperf(p.max_perf_pct);
            *app.dirty.borrow_mut() = false;
            report(&ui, &errs, &format!("applied '{}'", p.name));
        }
    });

    ui.on_save_profile({
        let app = app.clone();
        let w = ui.as_weak();
        let refresh = refresh_profiles.clone();
        move |name| {
            let ui = w.unwrap();
            let name = name.to_string();
            let prof = Profile {
                name: name.clone(),
                curve: app.draft.borrow().clone(),
                pl1: ui.get_edit_pl1(),
                pl2: ui.get_edit_pl2(),
                tau: ui.get_edit_tau(),
                powermode: 0,
                undervolt_mv: ui.get_edit_uv(),
                max_perf_pct: ui.get_edit_maxperf(),
                fan_fullspeed: app.hw.telemetry().fan_fullspeed,
                gpu_ctgp: ui.get_edit_ctgp(),
                gpu_ppab: ui.get_edit_ppab(),
                power_plan: None,
                cpu_temp_limit: ui.get_edit_cpu_temp(),
                gpu_temp_limit: ui.get_edit_gpu_temp(),
                gpu_boost: ui.get_edit_gpu_boost(),
                gpu_target_offset: ui.get_edit_gpu_offset(),
                kbd_backlight: ui.get_edit_kbd(),
                refresh_hz: app.hw.extras().refresh_hz,
                builtin: false,
            };
            {
                let mut cfg = app.cfg.borrow_mut();
                if let Some(existing) = cfg.profiles.iter_mut().find(|p| p.name == name && !p.builtin) {
                    *existing = prof;
                } else {
                    cfg.profiles.push(prof);
                }
            }
            let res = profiles::save(&app.cfg.borrow());
            refresh(&ui);
            match res {
                Ok(()) => {
                    ui.set_status_error(false);
                    ui.set_status(format!("saved '{name}'").into());
                }
                Err(e) => {
                    ui.set_status_error(true);
                    ui.set_status(e.into());
                }
            }
        }
    });

    ui.on_update_profile({
        let app = app.clone();
        let w = ui.as_weak();
        let refresh = refresh_profiles.clone();
        move |idx| {
            let ui = w.unwrap();
            let i = idx as usize;
            let name;
            {
                let mut cfg = app.cfg.borrow_mut();
                match cfg.profiles.get_mut(i) {
                    Some(p) if p.builtin => {
                        ui.set_status_error(true);
                        ui.set_status("built-in profiles cannot be edited - use 'save as' to make your own".into());
                        return;
                    }
                    Some(p) => {
                        // capture everything currently on screen and in hardware
                        p.curve = app.draft.borrow().clone();
                        p.pl1 = ui.get_edit_pl1();
                        p.pl2 = ui.get_edit_pl2();
                        p.tau = ui.get_edit_tau();
                        p.undervolt_mv = ui.get_edit_uv();
                        p.max_perf_pct = ui.get_edit_maxperf();
                        p.gpu_ctgp = ui.get_edit_ctgp();
                        p.gpu_ppab = ui.get_edit_ppab();
                        p.fan_fullspeed = app.hw.telemetry().fan_fullspeed;
                        p.powermode = app.hw.telemetry().powermode;
                        p.cpu_temp_limit = ui.get_edit_cpu_temp();
                        p.gpu_temp_limit = ui.get_edit_gpu_temp();
                        p.gpu_boost = ui.get_edit_gpu_boost();
                        p.gpu_target_offset = ui.get_edit_gpu_offset();
                        p.kbd_backlight = ui.get_edit_kbd();
                        p.refresh_hz = app.hw.extras().refresh_hz;
                        name = p.name.clone();
                    }
                    None => return,
                }
                let _ = profiles::save(&cfg);
            }
            refresh(&ui);
            ui.set_status_error(false);
            ui.set_status(format!("updated '{name}' with the current settings").into());
        }
    });

    ui.on_delete_profile({
        let app = app.clone();
        let w = ui.as_weak();
        let refresh = refresh_profiles.clone();
        move |idx| {
            let ui = w.unwrap();
            let i = idx as usize;
            let mut cfg = app.cfg.borrow_mut();
            if let Some(p) = cfg.profiles.get(i) {
                if p.builtin {
                    ui.set_status_error(true);
                    ui.set_status("built-in profiles cannot be deleted".into());
                    return;
                }
                let name = p.name.clone();
                cfg.profiles.remove(i);
                cfg.bindings.retain(|(_, n)| *n != name);
                let _ = profiles::save(&cfg);
                drop(cfg);
                refresh(&ui);
                ui.set_status_error(false);
                ui.set_status(format!("deleted '{name}'").into());
            }
        }
    });

    ui.on_bind_profile({
        let app = app.clone();
        let w = ui.as_weak();
        let refresh = refresh_profiles.clone();
        move |idx, mode_idx| {
            let ui = w.unwrap();
            let name = match app.cfg.borrow().profiles.get(idx as usize) {
                Some(p) => p.name.clone(),
                None => return,
            };
            {
                let mut cfg = app.cfg.borrow_mut();
                cfg.bindings.retain(|(_, n)| *n != name);
                // index 0 in the combo is "(no binding)"
                if mode_idx > 0 {
                    if let Some(m) = profiles::MODES.get(mode_idx as usize - 1) {
                        cfg.bindings.retain(|(mm, _)| mm != m);
                        cfg.bindings.push((*m, name.clone()));
                    }
                }
                let _ = profiles::save(&cfg);
            }
            refresh(&ui);
            ui.set_status_error(false);
            ui.set_status(if mode_idx > 0 {
                format!("'{}' now runs in {} mode", name,
                        profiles::mode_name(profiles::MODES[mode_idx as usize - 1])).into()
            } else {
                SharedString::from(format!("binding cleared for '{name}'"))
            });
        }
    });

    // ---- poll ----
    let timer = slint::Timer::default();
    {
        let app = app.clone();
        let w = ui.as_weak();
        // Start at a sentinel so the first tick treats the current mode as a
        // transition. Otherwise a profile bound to the mode you are already in
        // never fires, because there is no change to detect.
        let mut last_mode = i32::MIN;
        // None so the first tick counts as a transition and applies the binding
        let mut last_batt: Option<bool> = None;
        let mut last_plan: Option<String> = None;
        // extras() shells out to nvidia-smi and kscreen-doctor, so it runs on a
        // slower cadence than the sysfs telemetry.
        let mut tick: u32 = 0;
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(1500),
            move || {
                let ui = match w.upgrade() {
                    Some(u) => u,
                    None => return,
                };
                let t = app.hw.telemetry();
                ui.set_stats(Stats {
                    cpu_temp: t.cpu_temp,
                    gpu_temp: t.gpu_temp,
                    fan_rpm: t.fan_rpm,
                    fan_max: t.fan_max,
                    cpu_mhz: t.cpu_mhz,
                    gpu_watts: t.gpu_watts,
                    pl1: t.pl1,
                    pl2: t.pl2,
                    tau: t.tau,
                    powermode: t.powermode,
                    mode_name: profiles::mode_name(t.powermode).into(),
                    prochot: t.prochot,
                    undervolt_mv: t.undervolt_mv,
                    max_perf_pct: t.max_perf_pct,
                    on_battery: t.on_battery,
                });

                // A power mode change outside this app (keyboard shortcut,
                // another tool) reloads the EC table, so a bound profile has to
                // be re-applied and the displayed curve refreshed.
                if t.powermode != last_mode {
                    last_mode = t.powermode;
                    let bound = app
                        .cfg
                        .borrow()
                        .bindings
                        .iter()
                        .find(|(m, _)| *m == t.powermode)
                        .map(|(_, n)| n.clone());
                    if let Some(name) = bound {
                        let p = app.cfg.borrow().profiles.iter().find(|p| p.name == name).cloned();
                        if let Some(p) = p {
                            app.hw.set_power(p.pl1, p.pl2, p.tau);
                            app.hw.write_curve(&p.curve);
                            ui.set_edit_pl1(p.pl1);
                            ui.set_edit_pl2(p.pl2);
                            ui.set_edit_tau(p.tau);
                            ui.set_status_error(false);
                            ui.set_status(format!("mode changed — applied '{}'", p.name).into());
                        }
                    }
                    *app.dirty.borrow_mut() = false;
                }

                // Charger plugged or unplugged: apply whichever profile is bound
                // to the new power source. Watches the sysfs AC node directly
                // rather than PowerDevil, so it works regardless of desktop.
                if last_batt != Some(t.on_battery) {
                    last_batt = Some(t.on_battery);
                    let want = {
                        let cfg = app.cfg.borrow();
                        if t.on_battery { cfg.on_battery_profile.clone() } else { cfg.on_ac_profile.clone() }
                    };
                    if let Some(name) = want {
                        let p = app.cfg.borrow().profiles.iter().find(|p| p.name == name).cloned();
                        if let Some(p) = p {
                            if p.powermode != 0 {
                                let _ = app.hw.set_powermode(p.powermode);
                                std::thread::sleep(std::time::Duration::from_millis(600));
                            }
                            app.hw.set_power(p.pl1, p.pl2, p.tau);
                            let _ = app.hw.set_undervolt(p.undervolt_mv);
                            let _ = app.hw.set_max_perf(p.max_perf_pct);
                            apply_profile_extras(&app.hw, &p);
                            app.hw.write_curve(&p.curve);
                            let e = app.hw.extras();
                            seed_extras(&ui, &e);
                            ui.set_extras(to_extras(&e));
                            ui.set_edit_pl1(p.pl1);
                            ui.set_edit_pl2(p.pl2);
                            ui.set_edit_tau(p.tau);
                            ui.set_edit_uv(p.undervolt_mv);
                            ui.set_edit_maxperf(p.max_perf_pct);
                            ui.set_status_error(false);
                            ui.set_status(format!("{} - applied '{}'",
                                if t.on_battery { "on battery" } else { "on AC" }, p.name).into());
                        }
                    }
                    *app.dirty.borrow_mut() = false;
                }

                // KDE / power-profiles-daemon plan changed: apply any profile
                // bound to the new plan. Independent of the AC binding above so
                // both can be used together.
                let plan_now = app.hw.power_plan();
                if plan_now.is_some() && plan_now != last_plan {
                    last_plan = plan_now.clone();
                    let plan = plan_now.unwrap_or_default();
                    let p = app.cfg.borrow().profiles.iter()
                        .find(|p| p.power_plan.as_deref() == Some(plan.as_str())).cloned();
                    if let Some(p) = p {
                        if p.powermode != 0 {
                            let _ = app.hw.set_powermode(p.powermode);
                            std::thread::sleep(std::time::Duration::from_millis(600));
                        }
                        app.hw.set_power(p.pl1, p.pl2, p.tau);
                        let _ = app.hw.set_undervolt(p.undervolt_mv);
                        let _ = app.hw.set_max_perf(p.max_perf_pct);
                        apply_profile_extras(&app.hw, &p);
                        app.hw.write_curve(&p.curve);
                        if p.fan_fullspeed { let _ = app.hw.set_fan_fullspeed(true); }
                        let e = app.hw.extras();
                        seed_extras(&ui, &e);
                        ui.set_extras(to_extras(&e));
                        ui.set_edit_pl1(p.pl1);
                        ui.set_edit_pl2(p.pl2);
                        ui.set_edit_tau(p.tau);
                        ui.set_edit_uv(p.undervolt_mv);
                        ui.set_edit_maxperf(p.max_perf_pct);
                        ui.set_status_error(false);
                        ui.set_status(format!("KDE plan '{plan}' - applied '{}'", p.name).into());
                        *app.dirty.borrow_mut() = false;
                    }
                }

                tick += 1;
                if tick % 8 == 0 {
                    // Read-only display fields only. The editable widgets are
                    // seeded on write, never here.
                    ui.set_extras(to_extras(&app.hw.extras()));
                    ui.set_gpu_clients(ModelRc::new(VecModel::from(
                        hw::gpu_clients().into_iter().map(SharedString::from).collect::<Vec<_>>(),
                    )));
                }

                // Never clobber a curve the user is midway through editing.
                if !*app.dirty.borrow() {
                    let live = app.hw.read_curve();
                    if *app.draft.borrow() != live {
                        *app.draft.borrow_mut() = live.clone();
                        ui.set_curve(to_curve_model(&live));
                    }
                }
            },
        );
    }

    ui.run()?;
    Ok(())
}
