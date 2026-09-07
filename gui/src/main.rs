mod hw;
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
}

fn to_curve_model(pts: &[(i32, i32)]) -> ModelRc<CurvePoint> {
    ModelRc::new(VecModel::from(
        pts.iter().map(|(t, r)| CurvePoint { temp: *t, rpm: *r }).collect::<Vec<_>>(),
    ))
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

    *app.draft.borrow_mut() = app.hw.read_curve();
    ui.set_curve(to_curve_model(&app.draft.borrow()));

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
                    SharedString::from(format!("{}{}", p.name, bound))
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
            errs.extend(app.hw.write_curve(&p.curve));

            let live = app.hw.read_curve();
            *app.draft.borrow_mut() = live.clone();
            ui.set_curve(to_curve_model(&live));
            ui.set_edit_pl1(p.pl1);
            ui.set_edit_pl2(p.pl2);
            ui.set_edit_tau(p.tau);
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
