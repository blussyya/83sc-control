#!/usr/bin/env python3
import argparse
import csv
import datetime as dt
import os
import re
import shutil
import signal
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RUNS = os.path.join(ROOT, "data", "runs")
HELPER = "/usr/local/lib/83sc-control/helper.py"
TT = "/sys/devices/system/cpu/cpu0/thermal_throttle"

_tty = sys.stdout.isatty()
def c(k, s): return f"\033[{k}m{s}\033[0m" if _tty else str(s)
B = lambda s: c("1", s); D = lambda s: c("2", s)
R = lambda s: c("31", s); G = lambda s: c("32", s); Y = lambda s: c("33", s)

NV_ENV = {"__NV_PRIME_RENDER_OFFLOAD": "1", "__GLX_VENDOR_LIBRARY_NAME": "nvidia",
          "__VK_LAYER_NV_optimus": "NVIDIA_only"}

GPU_FIELDS = ("temperature.gpu,power.draw,clocks.current.graphics,utilization.gpu,"
              "clocks_throttle_reasons.active,clocks_throttle_reasons.hw_thermal_slowdown,"
              "clocks_throttle_reasons.sw_thermal_slowdown,"
              "clocks_throttle_reasons.hw_power_brake_slowdown,"
              "clocks_throttle_reasons.sw_power_cap")

def read(p, d=None):
    try:
        with open(p) as f:
            return f.read().strip()
    except OSError:
        return d

def num(v, d=0):
    try:
        return int(v)
    except (TypeError, ValueError):
        return d

def hwmon(name):
    base = "/sys/class/hwmon"
    for e in sorted(os.listdir(base)):
        p = os.path.join(base, e)
        if read(os.path.join(p, "name")) == name:
            return p
    return None

def gpu_sample():
    if not shutil.which("nvidia-smi"):
        return {}
    r = subprocess.run(["nvidia-smi", f"--query-gpu={GPU_FIELDS}",
                        "--format=csv,noheader,nounits"], capture_output=True, text=True)
    if r.returncode != 0 or not r.stdout.strip():
        return {}
    parts = [p.strip() for p in r.stdout.strip().splitlines()[0].split(",")]
    keys = ["temp", "watts", "mhz", "util", "thr_active", "thr_hw_thermal",
            "thr_sw_thermal", "thr_hw_power", "thr_sw_power"]
    out = dict(zip(keys, parts))
    return {k: (None if v.startswith("[") else v) for k, v in out.items()}

def start_cpu(seconds):
    n = os.cpu_count() or 8
    return subprocess.Popen(["stress-ng", "--cpu", str(n), "--timeout", f"{seconds}s"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def start_gpu(seconds):
    if not (os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY")):
        print(Y("  no display session -- skipping GPU load (run from your desktop)"))
        return None
    exe = shutil.which("vkcube") or shutil.which("glxgears")
    if not exe:
        print(Y("  no vkcube/glxgears found -- skipping GPU load"))
        return None
    env = {**os.environ, **NV_ENV}
    try:
        return subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL)
    except OSError as e:
        print(Y(f"  could not start GPU load: {e}"))
        return None

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cpu", action="store_true", help="synthetic CPU load")
    ap.add_argument("--gpu", action="store_true", help="synthetic GPU load (needs a display)")
    ap.add_argument("--monitor", action="store_true", help="no load; you run the game")
    ap.add_argument("--minutes", type=float, default=5)
    ap.add_argument("--label", default=None)
    ap.add_argument("--interval", type=float, default=2.0)
    a = ap.parse_args()
    if not (a.cpu or a.gpu or a.monitor):
        sys.exit("give --cpu and/or --gpu, or --monitor")

    secs = int(a.minutes * 60)
    legion = hwmon("legion_hwmon")
    coretemp = hwmon("coretemp")
    label = a.label or ("monitor" if a.monitor else
                        ("cpu+gpu" if a.cpu and a.gpu else "cpu" if a.cpu else "gpu"))
    label = re.sub(r"[^A-Za-z0-9._-]", "_", label)

    procs = []
    if a.cpu:
        procs.append(start_cpu(secs))
    if a.gpu:
        p = start_gpu(secs)
        if p:
            procs.append(p)

    base_c = num(read(f"{TT}/package_throttle_count"))
    base_ms = num(read(f"{TT}/package_throttle_total_time_ms"))
    gpu_throttled = set()
    rows = []
    t0 = time.monotonic()

    print(B(f"\n  {label}  --  {a.minutes:g} min"))
    if a.monitor:
        print(D("  start your match now; this only observes"))
    print(D(f"  {'time':>6} {'cpuC':>5} {'MHz':>6} {'fan':>6} {'PROCHOT':>8} "
            f"{'gpuC':>5} {'gpuW':>6} {'gpuMHz':>7} {'gpu throttle':>14}"))

    try:
        while time.monotonic() - t0 < secs:
            el = time.monotonic() - t0
            freqs = []
            for cpu in os.listdir("/sys/devices/system/cpu"):
                p = f"/sys/devices/system/cpu/{cpu}/cpufreq/scaling_cur_freq"
                if os.path.exists(p):
                    v = num(read(p))
                    if v:
                        freqs.append(v)
            avg = sum(freqs) / len(freqs) / 1000 if freqs else 0
            cput = num(read(f"{coretemp}/temp1_input")) // 1000 if coretemp else 0
            fan = num(read(f"{legion}/fan1_input")) if legion else 0
            pc = num(read(f"{TT}/package_throttle_count")) - base_c
            pms = num(read(f"{TT}/package_throttle_total_time_ms")) - base_ms
            g = gpu_sample()

            thr = []
            for k, name in (("thr_hw_thermal", "hw-thermal"), ("thr_sw_thermal", "sw-thermal"),
                            ("thr_hw_power", "hw-power"), ("thr_sw_power", "sw-power")):
                if (g.get(k) or "").lower().startswith(("active", "true", "1")):
                    thr.append(name)
                    gpu_throttled.add(name)
            thrs = ",".join(thr) if thr else "-"

            tc = R if cput >= 90 else Y if cput >= 80 else G
            pcc = R if pc else G
            print(f"  {el:>6.0f} {tc(f'{cput:>4}C')} {avg:>6.0f} {fan:>6} {pcc(f'{pc:>8}')} "
                  f"{(g.get('temp') or '-'):>5} {(g.get('watts') or '-'):>6} "
                  f"{(g.get('mhz') or '-'):>7} {thrs:>14}")

            rows.append({"t": round(el, 1), "cpu_c": cput, "cpu_mhz": round(avg),
                         "fan_rpm": fan, "prochot": pc, "prochot_ms": pms,
                         "gpu_c": g.get("temp"), "gpu_w": g.get("watts"),
                         "gpu_mhz": g.get("mhz"), "gpu_util": g.get("util"),
                         "gpu_throttle": thrs})
            time.sleep(a.interval)
    except KeyboardInterrupt:
        print(D("\n  stopped"))
    finally:
        for p in procs:
            try:
                p.send_signal(signal.SIGTERM)
            except (OSError, ProcessLookupError):
                pass

    if not rows:
        return
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    outdir = os.path.join(RUNS, f"{stamp}-stress-{label}")
    os.makedirs(outdir, exist_ok=True)
    with open(os.path.join(outdir, "samples.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)

    temps = [r["cpu_c"] for r in rows if r["cpu_c"]]
    gw = [float(r["gpu_w"]) for r in rows if r["gpu_w"]]
    print(B("\n  summary"))
    print(f"    cpu max/avg      {max(temps)}C / {sum(temps)//len(temps)}C")
    print(f"    PROCHOT events   {rows[-1]['prochot']}   total {rows[-1]['prochot_ms']}ms")
    if gw:
        print(f"    gpu power max    {max(gw):.1f}W")
        if max(gw) < 10:
            print(Y("    GPU never loaded -- this was effectively a CPU-only run"))
    print(f"    gpu throttling   {', '.join(sorted(gpu_throttled)) if gpu_throttled else G('none detected')}")
    print(D("    (50-series hotspot sensor is disabled by NVIDIA; throttle-reason"))
    print(D("     bits above are the reliable signal, not edge temperature)"))
    print(D(f"\n  saved -> {os.path.relpath(outdir, ROOT)}\n"))

if __name__ == "__main__":
    main()
