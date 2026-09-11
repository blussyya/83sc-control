use std::collections::VecDeque;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

const DEFAULT_TIMEOUT_SECS: u64 = 5;

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// The LED sysfs node is root-only and there is no uaccess rule for it, so
/// UPower is the one path that works from a plain user session. It also
/// broadcasts every change with its origin, which is what lets this daemon
/// tell a Fn+Space press or a KDE write apart from its own.
#[zbus::proxy(
    interface = "org.freedesktop.UPower.KbdBacklight",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower/KbdBacklight"
)]
trait KbdBacklight {
    fn get_brightness(&self) -> zbus::Result<i32>;
    fn set_brightness(&self, value: i32) -> zbus::Result<()>;
    #[zbus(signal)]
    fn brightness_changed_with_source(&self, value: i32, source: String) -> zbus::Result<()>;
}

enum Ev {
    Idle,
    Resume,
    Changed { value: i32, source: String },
}

struct Wl {
    tx: Sender<Ev>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Wl {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for Wl {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtIdleNotifierV1, ()> for Wl {
    fn event(_: &mut Self, _: &ExtIdleNotifierV1, _: <ExtIdleNotifierV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtIdleNotificationV1, ()> for Wl {
    fn event(state: &mut Self, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let ev = match event {
            ext_idle_notification_v1::Event::Idled => Ev::Idle,
            ext_idle_notification_v1::Event::Resumed => Ev::Resume,
            _ => return,
        };
        let _ = state.tx.send(ev);
    }
}

fn config_path() -> PathBuf {
    let base = env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(".config"));
    base.join("83sc-control").join("kbd-idle.conf")
}

fn timeout_secs() -> u64 {
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--timeout" {
            if let Some(v) = args.next().and_then(|v| v.parse().ok()) {
                return v;
            }
        }
    }
    fs::read_to_string(config_path())
        .ok()
        .and_then(|s| {
            s.lines()
                .filter_map(|l| l.split_once('='))
                .find(|(k, _)| k.trim() == "timeout")
                .and_then(|(_, v)| v.trim().parse().ok())
        })
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

fn spawn_wayland(tx: Sender<Ev>, timeout_ms: u32) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("wayland: {e}"))?;
    let (globals, mut queue) = registry_queue_init::<Wl>(&conn).map_err(|e| format!("wayland registry: {e}"))?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("wl_seat: {e}"))?;
    let notifier: ExtIdleNotifierV1 = globals
        .bind(&qh, 1..=2, ())
        .map_err(|e| format!("compositor has no ext_idle_notifier_v1: {e}"))?;

    // v2 adds the input-only variant, which ignores idle inhibitors. A video
    // player holding an inhibitor should keep the screen on, not the keyboard.
    let notification = if wayland_client::Proxy::version(&notifier) >= 2 {
        notifier.get_input_idle_notification(timeout_ms, &seat, &qh, ())
    } else {
        notifier.get_idle_notification(timeout_ms, &seat, &qh, ())
    };

    thread::spawn(move || {
        let _keep = notification;
        let mut state = Wl { tx };
        loop {
            if queue.blocking_dispatch(&mut state).is_err() {
                exit(1);
            }
        }
    });
    Ok(())
}

struct Backlight {
    proxy: KbdBacklightProxyBlocking<'static>,
    level: i32,
    dimmed: bool,
    own: VecDeque<i32>,
    kde_dimmed: bool,
    reassert_until: Option<Instant>,
}

impl Backlight {
    fn set(&mut self, v: i32) {
        self.own.push_back(v);
        if let Err(e) = self.proxy.set_brightness(v) {
            eprintln!("set_brightness({v}): {e}");
            self.own.pop_back();
        }
    }

    fn on_idle(&mut self) {
        let cur = match self.proxy.get_brightness() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("get_brightness: {e}");
                return;
            }
        };
        if cur > 0 {
            eprintln!("idle: off (was {cur})");
            self.level = cur;
            self.dimmed = true;
            self.set(0);
        }
    }

    fn on_resume(&mut self) {
        if !self.dimmed {
            return;
        }
        eprintln!("resume: {}", self.level);
        self.dimmed = false;
        self.set(self.level);
        // KDE's DimDisplay saves the current level when it dims. If we got there
        // first it saved 0 and will "restore" 0 on wake, racing us. Expect one
        // such write and put the level back once.
        if self.kde_dimmed {
            self.kde_dimmed = false;
            self.reassert_until = Some(Instant::now() + Duration::from_secs(2));
        }
    }

    fn on_changed(&mut self, value: i32, source: &str) {
        if self.own.front() == Some(&value) {
            self.own.pop_front();
            return;
        }
        if source == "internal" {
            eprintln!("Fn key set {value}");
            self.level = value;
            self.dimmed = false;
            self.kde_dimmed = false;
            self.reassert_until = None;
            return;
        }
        if self.dimmed {
            if value == 0 {
                self.kde_dimmed = true;
            } else {
                self.dimmed = false;
                self.level = value;
            }
            return;
        }
        let racing = self.reassert_until.map_or(false, |t| Instant::now() < t);
        if value == 0 && racing {
            self.reassert_until = None;
            self.set(self.level);
        } else {
            self.level = value;
        }
    }
}

fn main() {
    let secs = timeout_secs();
    let (tx, rx) = mpsc::channel();

    let conn = match zbus::blocking::Connection::system() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("system bus: {e}");
            exit(1);
        }
    };
    let proxy = match KbdBacklightProxyBlocking::new(&conn) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("UPower KbdBacklight: {e}");
            exit(1);
        }
    };
    let level = match proxy.get_brightness() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("no keyboard backlight via UPower: {e}");
            exit(1);
        }
    };

    let signals = match proxy.receive_brightness_changed_with_source() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("signal subscribe: {e}");
            exit(1);
        }
    };
    let stx = tx.clone();
    thread::spawn(move || {
        for sig in signals {
            if let Ok(a) = sig.args() {
                let _ = stx.send(Ev::Changed { value: *a.value(), source: a.source().clone() });
            }
        }
        exit(1);
    });

    if let Err(e) = spawn_wayland(tx, (secs * 1000) as u32) {
        eprintln!("{e}");
        exit(1);
    }

    eprintln!("83sc-kbd-idle: off after {secs}s idle, current level {level}");

    unsafe {
        libc::signal(libc::SIGTERM, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGHUP, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }

    let mut bl = Backlight { proxy, level, dimmed: false, own: VecDeque::new(), kde_dimmed: false, reassert_until: None };
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ev::Idle) => bl.on_idle(),
            Ok(Ev::Resume) => bl.on_resume(),
            Ok(Ev::Changed { value, source }) => bl.on_changed(value, &source),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if STOP.load(Ordering::SeqCst) {
            break;
        }
    }
    // Being stopped while dimmed must not leave the keyboard dark with nothing
    // left running to bring it back.
    if bl.dimmed {
        bl.set(bl.level);
    }
}
