#!/usr/bin/env python3
import argparse
import csv
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
RUNS = os.path.join(ROOT, "data", "runs")
CLI = os.path.join(ROOT, "bin", "83sc")
HELPER = "/usr/local/lib/83sc-control/helper.py"
RAPL = "/sys/class/powercap/intel-rapl:0"
MANGO_DIRS = [os.path.expanduser("~/mangologs"), os.path.expanduser("~"),
              os.path.expanduser("~/.config/MangoHud")]

_tty = sys.stdout.isatty()
def c(code, s): return f"\033[{code}m{s}\033[0m" if _tty else str(s)
BOLD = lambda s: c("1", s); DIM = lambda s: c("2", s)
RED = lambda s: c("31", s); GRN = lambda s: c("32", s); YEL = lambda s: c("33", s)

def read(path):
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError:
        return None

def num(v, default=None):
    try:
        return int(v)
    except (TypeError, ValueError):
        return default

def hwmon(name):
    base = "/sys/class/hwmon"
    for e in sorted(os.listdir(base)):
        p = os.path.join(base, e)
        if read(os.path.join(p, "name")) == name:
            return p
    return None

def helper(*args):
    r = subprocess.run(["sudo", "-n", HELPER, *args], capture_output=True, text=True)
    return r

class Sampler(threading.Thread):

    def __init__(self, path, interval=1.0):
        super().__init__(daemon=True)
        self.path, self.interval = path, interval
        self.stop_flag = threading.Event()
        self.rows = []
        self.legion = hwmon("legion_hwmon")
        self.coretemp = hwmon("coretemp")
        self.cpus = sorted(
            d for d in os.listdir("/sys/devices/system/cpu")
            if re.fullmatch(r"cpu\d+", d)
            and os.path.exists(f"/sys/devices/system/cpu/{d}/cpufreq/scaling_cur_freq"))

    def _energy(self):
        return num(read(f"{RAPL}/energy_uj"), 0)

    def _throttle(self):
        tt = "/sys/devices/system/cpu/cpu0/thermal_throttle"
        return (num(read(f"{tt}/package_throttle_count"), 0),
                num(read(f"{tt}/package_throttle_total_time_ms"), 0))

    def run(self):
        prev_e, prev_t = self._energy(), time.monotonic()
        t0 = prev_t
        while not self.stop_flag.is_set():
            time.sleep(self.interval)
            now = time.monotonic()
            e = self._energy()
            de, dt_s = e - prev_e, now - prev_t
            watts = (de / 1e6) / dt_s if de >= 0 and dt_s > 0 else None
            prev_e, prev_t = e, now

            freqs = [num(read(f"/sys/devices/system/cpu/{c_}/cpufreq/scaling_cur_freq"), 0) for c_ in self.cpus]
            freqs = [f for f in freqs if f]
            tc, tms = self._throttle()

            self.rows.append({
                "t": round(now - t0, 2),
                "pkg_temp_c": round(num(read(f"{self.coretemp}/temp1_input"), 0) / 1000.0, 1) if self.coretemp else None,
                "ec_cpu_temp_c": round(num(read(f"{self.legion}/temp1_input"), 0) / 1000.0, 1) if self.legion else None,
                "ec_gpu_temp_c": round(num(read(f"{self.legion}/temp2_input"), 0) / 1000.0, 1) if self.legion else None,
                "fan1_rpm": num(read(f"{self.legion}/fan1_input"), 0) if self.legion else None,
                "fan2_rpm": num(read(f"{self.legion}/fan2_input"), 0) if self.legion else None,
                "pkg_watts": round(watts, 2) if watts is not None else None,
                "freq_avg_mhz": round(sum(freqs) / len(freqs) / 1000.0, 0) if freqs else None,
                "freq_min_mhz": round(min(freqs) / 1000.0, 0) if freqs else None,
                "freq_max_mhz": round(max(freqs) / 1000.0, 0) if freqs else None,
                "prochot_count": tc,
                "prochot_ms": tms,
            })

    def save(self):
        if not self.rows:
            return
        with open(self.path, "w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=list(self.rows[0].keys()))
            w.writeheader()
            w.writerows(self.rows)

def apply_config(preset, sets):
    applied = []
    if preset:
        print(f"\n  {BOLD('config')}: preset {BOLD(preset)}")
        r = subprocess.run([CLI, "preset", preset], text=True)
        applied.append(("preset", preset, r.returncode == 0))
    for kv in sets:
        if "=" not in kv:
            sys.exit(f"--set expects target=value, got {kv!r}")
        k, v = kv.split("=", 1)
        r = subprocess.run([CLI, "set", k, v], capture_output=True, text=True)
        ok = r.returncode == 0
        print(f"    {'ok  ' if ok else RED('FAIL')} {k} = {v}")
        applied.append((k, v, ok))
    return applied

def newest_mangohud_csv(after_ts):
    best, best_m = None, 0
    for d in MANGO_DIRS:
        if not os.path.isdir(d):
            continue
        for fn in os.listdir(d):
            if not fn.endswith(".csv"):
                continue
            p = os.path.join(d, fn)
            try:
                m = os.path.getmtime(p)
            except OSError:
                continue
            if m >= after_ts and m > best_m:
                best, best_m = p, m
    return best

def analyse(rows):
    if not rows:
        return {}
    temps = [r["pkg_temp_c"] for r in rows if r["pkg_temp_c"]]
    watts = [r["pkg_watts"] for r in rows if r["pkg_watts"]]
    freqs = [r["freq_avg_mhz"] for r in rows if r["freq_avg_mhz"]]
    lows = [r for r in rows if r["freq_min_mhz"] and r["freq_min_mhz"] <= 500]
    total_pro = rows[-1]["prochot_count"] - rows[0]["prochot_count"]
    total_ms = rows[-1]["prochot_ms"] - rows[0]["prochot_ms"]

    active, streak, longest = 0, 0, 0
    for a, b in zip(rows, rows[1:]):
        if b["prochot_count"] > a["prochot_count"]:
            active += 1
            streak += 1
            longest = max(longest, streak)
        else:
            streak = 0

    return {
        "samples": len(rows),
        "duration_s": rows[-1]["t"],
        "temp_max_c": max(temps) if temps else None,
        "temp_avg_c": round(sum(temps) / len(temps), 1) if temps else None,
        "watts_max": max(watts) if watts else None,
        "watts_avg": round(sum(watts) / len(watts), 1) if watts else None,
        "freq_avg_mhz": round(sum(freqs) / len(freqs)) if freqs else None,
        "floor_samples": len(lows),
        "floor_pct": round(100.0 * len(lows) / len(rows), 1),
        "prochot_events": total_pro,
        "prochot_total_ms": total_ms,
        "prochot_active_windows": active,
        "prochot_longest_streak_s": longest,
    }

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--preset")
    ap.add_argument("--set", action="append", default=[], dest="sets")
    ap.add_argument("--minutes", type=float, default=12)
    ap.add_argument("--label", default=None)
    ap.add_argument("--interval", type=float, default=1.0)
    a = ap.parse_args()

    if not os.path.exists(HELPER):
        sys.exit("harness: helper not installed. Run:  sudo ./install.sh")
    if not a.preset and not a.sets:
        sys.exit("harness: give --preset and/or --set target=value")

    label = a.label or a.preset or "-".join(s.replace("=", "") for s in a.sets)
    label = re.sub(r"[^A-Za-z0-9._-]", "_", label)
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    outdir = os.path.join(RUNS, f"{stamp}-{label}")
    os.makedirs(outdir, exist_ok=True)

    print(BOLD(f"\n  === run: {label} ==="))
    applied = apply_config(a.preset, a.sets)

    before = json.loads(helper("status").stdout or "{}")
    with open(os.path.join(outdir, "state-before.json"), "w") as f:
        json.dump(before, f, indent=2)

    print(f"\n  {BOLD('Now play a real match')} for {BOLD(f'{a.minutes:g} minutes')}.")
    print(DIM("  A real match, not a bot game or the menu -- thermal soak only shows up"))
    print(DIM("  under sustained load. Note roughly when it feels bad, if it does."))
    input(f"\n  {BOLD('Press Enter when CS2 is running and you are in the match...')}")

    start_ts = time.time()
    csv_path = os.path.join(outdir, "samples.csv")
    s = Sampler(csv_path, a.interval)
    s.start()

    deadline = time.time() + a.minutes * 60
    try:
        while time.time() < deadline:
            left = int(deadline - time.time())
            last = s.rows[-1] if s.rows else {}
            t = last.get("pkg_temp_c") or 0
            tcol = RED if t >= 90 else YEL if t >= 80 else GRN
            print(f"\r  {left//60:02d}:{left%60:02d} left   "
                  f"temp {tcol(f'{t:.0f}C')}   {last.get('pkg_watts') or 0:.0f}W   "
                  f"{last.get('freq_avg_mhz') or 0:.0f}MHz   "
                  f"fan {last.get('fan1_rpm') or 0}/{last.get('fan2_rpm') or 0}   "
                  f"PROCHOT {last.get('prochot_count', 0) - (s.rows[0]['prochot_count'] if s.rows else 0)}   ",
                  end="", flush=True)
            time.sleep(1)
    except KeyboardInterrupt:
        print(DIM("\n  cut short by user"))
    finally:
        s.stop_flag.set()
        s.join(timeout=5)
        s.save()
    print()

    summary = analyse(s.rows)
    print(f"\n  {BOLD('measured')}")
    for k, v in summary.items():
        print(f"    {k:<28} {v}")

    print(f"\n  {BOLD('How did it actually feel?')}")
    print(DIM("  This matters as much as the numbers -- the counters cannot tell"))
    print(DIM("  a drop between rounds from one mid-gunfight."))
    rating = input("    smooth / ok / bad  : ").strip().lower() or "unrated"
    when = ""
    if rating.startswith("b") or rating.startswith("o"):
        when = input("    roughly when / doing what? : ").strip()
    notes = input("    any other notes (Enter to skip): ").strip()

    mango = newest_mangohud_csv(start_ts)
    if mango:
        dest = os.path.join(outdir, "mangohud.csv")
        try:
            shutil.copy2(mango, dest)
            print(DIM(f"\n  captured MangoHud log: {os.path.basename(mango)}"))
        except OSError as e:
            print(DIM(f"\n  could not copy MangoHud log: {e}"))

    after = json.loads(helper("status").stdout or "{}")
    with open(os.path.join(outdir, "state-after.json"), "w") as f:
        json.dump(after, f, indent=2)

    meta = {
        "label": label, "timestamp": stamp, "minutes": a.minutes,
        "preset": a.preset, "sets": a.sets,
        "applied": [{"target": t, "value": v, "ok": ok} for t, v, ok in applied],
        "summary": summary,
        "subjective": {"rating": rating, "when": when, "notes": notes},
        "mangohud_source": mango,
        "config_snapshot": {
            "profile": before.get("profile"),
            "rapl": before.get("rapl"),
            "fwattr": before.get("fwattr"),
            "cpufreq": before.get("cpufreq"),
            "fan_fullspeed": before.get("legion", {}).get("fan_fullspeed"),
        },
    }
    with open(os.path.join(outdir, "meta.json"), "w") as f:
        json.dump(meta, f, indent=2)

    print(f"\n  {GRN('saved')} -> {os.path.relpath(outdir, ROOT)}")
    print(DIM("  compare runs with:  ./harness/compare.py\n"))

if __name__ == "__main__":
    main()
