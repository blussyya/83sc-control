#!/usr/bin/env python3
import json
import os
import re
import subprocess
import sys

LEGION = "/sys/bus/platform/devices/PNP0C09:00"
FWATTR = "/sys/class/firmware-attributes/lenovo-wmi-other-0/attributes"
RAPL = "/sys/class/powercap/intel-rapl:0"
PSTATE = "/sys/devices/system/cpu/intel_pstate"

WRITE_PREFIXES = (
    "/sys/bus/platform/devices/PNP0C09:00",
    "/sys/devices/pci0000:00/0000:00:1f.0/PNP0C09:00",
    "/sys/class/hwmon",
    "/sys/devices/platform",
    "/sys/class/powercap",
    "/sys/devices/virtual/powercap",
    "/sys/devices/system/cpu",
    "/sys/class/firmware-attributes",
    "/sys/devices/virtual/firmware-attributes",
    "/sys/firmware/acpi/platform_profile",
)

def die(msg, code=1):
    print(f"helper: {msg}", file=sys.stderr)
    sys.exit(code)

def read(path):
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError:
        return None

def write_raw(path, value):
    try:
        with open(path, "w") as f:
            f.write(str(value))
        return True, None
    except OSError as e:
        return False, str(e)

def hwmon(name):
    base = "/sys/class/hwmon"
    try:
        for entry in sorted(os.listdir(base)):
            path = os.path.join(base, entry)
            if read(os.path.join(path, "name")) == name:
                return path
    except OSError:
        pass
    return None

def check_path(path):
    real = os.path.realpath(path)
    for prefix in WRITE_PREFIXES:
        if real == prefix or real.startswith(prefix.rstrip("/") + "/"):
            return real
    die(f"path outside writable subsystems: {real}\n"
        f"        allowed prefixes:\n          " + "\n          ".join(WRITE_PREFIXES))

NAMED = {
    "pl1":            f"{FWATTR}/ppt_pl1_spl/current_value",
    "pl2":            f"{FWATTR}/ppt_pl2_sppt/current_value",
    "pl3":            f"{FWATTR}/ppt_pl3_fppt/current_value",

    "ec_pl1":         f"{LEGION}/cpu_longterm_powerlimit",
    "ec_pl2":         f"{LEGION}/cpu_shortterm_powerlimit",
    "ec_peak":        f"{LEGION}/cpu_peak_powerlimit",
    "ec_tau":         f"{LEGION}/cpu_l1_tau",
    "ec_crossload":   f"{LEGION}/cpu_cross_loading_powerlimit",
    "ec_pl_coupling": f"{LEGION}/cpu_pl_coupling",
    "cpu_temp_limit": f"{LEGION}/cpu_temperature_limit",
    "gpu_temp_limit": f"{LEGION}/gpu_temperature_limit",
    "gpu_ctgp":       f"{LEGION}/gpu_ctgp_powerlimit",
    "gpu_ppab":       f"{LEGION}/gpu_ppab_powerlimit",
    "gpu_offset":     f"{LEGION}/gpu_power_target_offset",
    "cpu_oc":         f"{LEGION}/cpu_oc",
    "gpu_oc":         f"{LEGION}/gpu_oc",

    "fan_full":       f"{LEGION}/fan_fullspeed",
    "fan_max":        f"{LEGION}/fan_maxspeed",
    "fan_unlock":     f"{LEGION}/fan_unlock",
    "lock_fan_ctl":   f"{LEGION}/lockfancontroller",

    "profile":        "/sys/firmware/acpi/platform_profile",
    "powermode":      f"{LEGION}/powermode",

    "rapl_pl1_uw":    f"{RAPL}/constraint_0_power_limit_uw",
    "rapl_pl2_uw":    f"{RAPL}/constraint_1_power_limit_uw",
    "rapl_tau_us":    f"{RAPL}/constraint_0_time_window_us",
    "rapl_enabled":   f"{RAPL}/enabled",

    "no_turbo":       f"{PSTATE}/no_turbo",
    "max_perf_pct":   f"{PSTATE}/max_perf_pct",
    "min_perf_pct":   f"{PSTATE}/min_perf_pct",

    "fn_lock":        f"{LEGION}/fn_lock",
    "touchpad":       f"{LEGION}/touchpad",
    "winkey":         f"{LEGION}/winkey",
    "battery_conserv": f"{LEGION}/battery_conservation",
    "rapid_charge":   f"{LEGION}/rapidcharge",
    "overdrive":      f"{LEGION}/overdrive",
    "igpumode":       f"{LEGION}/igpumode",
}

PERCPU = {
    "governor": "scaling_governor",
    "epp":      "energy_performance_preference",
    "freq_max": "scaling_max_freq",
    "freq_min": "scaling_min_freq",
}

def do_write(path, value, label=None):
    real = check_path(path)
    if not os.path.exists(real):
        die(f"{label or path}: {real} does not exist")
    before = read(real)
    ok, err = write_raw(real, value)
    after = read(real)
    result = {"target": label or path, "path": real, "requested": str(value),
              "before": before, "after": after, "write_ok": ok, "error": err}
    print(json.dumps(result))
    if not ok:
        return 3
    if after is not None and after.strip() != str(value).strip():
        print(f"helper: NOTE readback differs: wrote {value}, node reads {after} "
              f"(firmware clamped or ignored it)", file=sys.stderr)
        return 4
    return 0

def cmd_set(argv):
    if len(argv) != 2:
        die("usage: set <name|path> <value>   (see: targets)")
    target, value = argv

    if target in PERCPU:
        node = PERCPU[target]
        ok = failed = 0
        errs = []
        for cpu in sorted(os.listdir("/sys/devices/system/cpu")):
            if not re.fullmatch(r"cpu\d+", cpu):
                continue
            path = f"/sys/devices/system/cpu/{cpu}/cpufreq/{node}"
            if not os.path.exists(path):
                continue
            check_path(path)
            good, err = write_raw(path, value)
            if good:
                ok += 1
            else:
                failed += 1
                errs.append(f"{cpu}: {err}")
        print(json.dumps({"target": target, "value": value, "cpus_ok": ok,
                          "cpus_failed": failed, "errors": errs[:4]}))
        return 0 if failed == 0 else 3

    path = NAMED.get(target, target)
    return do_write(path, value, label=target)

def cmd_fancurve(argv):
    if "--unsafe" not in argv:
        die("fan-curve writes are gated: the EC offset table reads all-zero on this\n"
            "        model, so writes would target unverified registers. Pass --unsafe\n"
            "        to proceed anyway, or reverse the layout first (see acpidump).")
    argv = [a for a in argv if a != "--unsafe"]
    if len(argv) != 3:
        die("usage: fan-curve <point 1-10> <field> <value> --unsafe\n"
            "        field: fan1_pwm fan2_pwm cpu_temp gpu_temp cpu_hyst accel decel")
    point, field, value = argv
    if not re.fullmatch(r"([1-9]|10)", point):
        die("point must be 1-10")
    fields = {"fan1_pwm": "pwm1_auto_point{}_pwm", "fan2_pwm": "pwm2_auto_point{}_pwm",
              "cpu_temp": "pwm1_auto_point{}_temp", "gpu_temp": "pwm2_auto_point{}_temp",
              "cpu_hyst": "pwm1_auto_point{}_temp_hyst", "accel": "pwm1_auto_point{}_accel",
              "decel": "pwm1_auto_point{}_decel"}
    if field not in fields:
        die(f"field must be one of: {', '.join(fields)}")
    base = hwmon("legion_hwmon")
    if not base:
        die("legion_hwmon not found (is legion_laptop loaded?)")
    return do_write(os.path.join(base, fields[field].format(point)), value,
                    label=f"curve[{point}].{field}")

def cmd_status():
    lh = hwmon("legion_hwmon")
    out = {
        "dmi": {k: read(f"/sys/class/dmi/id/{k}")
                for k in ("product_name", "product_version", "bios_version", "bios_date", "board_name")},
        "profile": read("/sys/firmware/acpi/platform_profile"),
        "profile_choices": read("/sys/firmware/acpi/platform_profile_choices"),
        "legion": {}, "fwattr": {}, "rapl": {}, "cpufreq": {}, "thermal": {}, "fans": {}, "curve": [],
    }

    for node in ("powermode", "thermalmode", "cpu_longterm_powerlimit", "cpu_shortterm_powerlimit",
                 "cpu_peak_powerlimit", "cpu_l1_tau", "cpu_temperature_limit", "cpu_cross_loading_powerlimit",
                 "cpu_default_powerlimit", "cpu_pl_coupling", "cpu_oc", "gpu_temperature_limit",
                 "gpu_ctgp_powerlimit", "gpu_ppab_powerlimit", "gpu_power_target_offset", "gpu_boost_clock",
                 "gpu_oc", "fan_fullspeed", "fan_maxspeed", "fan_unlock", "lockfancontroller",
                 "battery_conservation", "rapidcharge", "overdrive", "fn_lock", "igpumode", "gsync",
                 "aslcodeversion", "powerchargemode"):
        out["legion"][node] = read(f"{LEGION}/{node}")

    for attr in ("ppt_pl1_spl", "ppt_pl2_sppt", "ppt_pl3_fppt"):
        out["fwattr"][attr] = {k: read(f"{FWATTR}/{attr}/{k}")
                               for k in ("current_value", "default_value", "min_value", "max_value")}

    for c in (0, 1, 2):
        name = read(f"{RAPL}/constraint_{c}_name")
        if name:
            out["rapl"][name] = {
                "power_limit_uw": read(f"{RAPL}/constraint_{c}_power_limit_uw"),
                "time_window_us": read(f"{RAPL}/constraint_{c}_time_window_us"),
            }
    out["rapl"]["enabled"] = read(f"{RAPL}/enabled")

    cpu0 = "/sys/devices/system/cpu/cpu0/cpufreq"
    out["cpufreq"] = {
        "driver": read(f"{cpu0}/scaling_driver"),
        "governor": read(f"{cpu0}/scaling_governor"),
        "epp": read(f"{cpu0}/energy_performance_preference"),
        "scaling_min_khz": read(f"{cpu0}/scaling_min_freq"),
        "scaling_max_khz": read(f"{cpu0}/scaling_max_freq"),
        "hw_min_khz": read(f"{cpu0}/cpuinfo_min_freq"),
        "hw_max_khz": read(f"{cpu0}/cpuinfo_max_freq"),
        "no_turbo": read(f"{PSTATE}/no_turbo"),
        "min_perf_pct": read(f"{PSTATE}/min_perf_pct"),
        "max_perf_pct": read(f"{PSTATE}/max_perf_pct"),
    }

    tt = "/sys/devices/system/cpu/cpu0/thermal_throttle"
    out["thermal"] = {k: read(f"{tt}/{k}") for k in
                      ("core_throttle_count", "core_throttle_total_time_ms",
                       "package_throttle_count", "package_throttle_total_time_ms")}
    uv = None
    try:
        r = subprocess.run(["/usr/bin/intel-undervolt", "read"], capture_output=True, text=True)
        m = re.search(r"^CPU \(0\): *(-?[\d.]+) mV", r.stdout or "", re.M)
        if m:
            uv = float(m.group(1))
    except OSError:
        pass
    out["undervolt_mv"] = uv
    out["on_battery"] = read("/sys/class/power_supply/ACAD/online") == "0"

    ct = hwmon("coretemp")
    if ct:
        out["thermal"]["package_temp_mc"] = read(f"{ct}/temp1_input")

    if lh:
        for i in (1, 2, 3):
            out["fans"][f"temp{i}"] = {"label": read(f"{lh}/temp{i}_label"),
                                       "value_mc": read(f"{lh}/temp{i}_input")}
        for i in (1, 2):
            out["fans"][f"fan{i}"] = {"rpm": read(f"{lh}/fan{i}_input"),
                                      "target": read(f"{lh}/fan{i}_target"),
                                      "max": read(f"{lh}/fan{i}_max")}
        for p in range(1, 11):
            out["curve"].append({
                "point": p,
                "fan1_pwm": read(f"{lh}/pwm1_auto_point{p}_pwm"),
                "fan2_pwm": read(f"{lh}/pwm2_auto_point{p}_pwm"),
                "cpu_temp": read(f"{lh}/pwm1_auto_point{p}_temp"),
                "cpu_hyst": read(f"{lh}/pwm1_auto_point{p}_temp_hyst"),
                "gpu_temp": read(f"{lh}/pwm2_auto_point{p}_temp"),
                "accel": read(f"{lh}/pwm1_auto_point{p}_accel"),
                "decel": read(f"{lh}/pwm1_auto_point{p}_decel"),
            })

    print(json.dumps(out, indent=2))

def cmd_targets():
    print(json.dumps({
        "named": {k: v for k, v in sorted(NAMED.items())},
        "percpu": {k: f"/sys/devices/system/cpu/cpu*/cpufreq/{v}" for k, v in sorted(PERCPU.items())},
        "note": "any path under the writable prefixes also works with 'set <path> <value>'",
        "writable_prefixes": list(WRITE_PREFIXES),
    }, indent=2))

def cmd_dump_ec():
    subprocess.run(["modprobe", "ec_sys", "write_support=0"], check=False)
    path = "/sys/kernel/debug/ec/ec0/io"
    if not os.path.exists(path):
        die("ec_sys did not expose /sys/kernel/debug/ec/ec0/io")
    with open(path, "rb") as f:
        data = f.read()
    print(json.dumps({"size": len(data), "hex": data.hex()}))

def cmd_ec_write(argv):
    if "--unsafe" not in argv:
        die("raw EC writes need --unsafe: a wrong offset can hit battery or fan\n"
            "        controller registers and leave the EC in a bad state.")
    argv = [a for a in argv if a != "--unsafe"]
    if len(argv) != 2:
        die("usage: ec-write <offset hex> <byte hex> --unsafe")
    off_s, val_s = argv
    if not re.fullmatch(r"(0x)?[0-9a-fA-F]{1,3}", off_s):
        die("offset must be hex 0x00-0xff")
    if not re.fullmatch(r"(0x)?[0-9a-fA-F]{1,2}", val_s):
        die("value must be a hex byte")
    off, val = int(off_s, 16), int(val_s, 16)
    if not (0 <= off <= 0xFF and 0 <= val <= 0xFF):
        die("offset and value must fit in a byte")
    subprocess.run(["modprobe", "ec_sys", "write_support=1"], check=False)
    path = "/sys/kernel/debug/ec/ec0/io"
    if not os.path.exists(path):
        die("ec_sys not available")
    try:
        fd = os.open(path, os.O_RDWR)
        try:
            os.lseek(fd, off, os.SEEK_SET)
            before = os.read(fd, 1)
            os.lseek(fd, off, os.SEEK_SET)
            os.write(fd, bytes([val]))
            os.lseek(fd, off, os.SEEK_SET)
            after = os.read(fd, 1)
        finally:
            os.close(fd)
    except OSError as e:
        die(f"EC write failed (is ec_sys loaded with write_support=1?): {e}")
    print(json.dumps({"offset": f"{off:#04x}", "before": before.hex(),
                      "wrote": f"{val:#04x}", "after": after.hex()}))

def cmd_msr(argv):
    if len(argv) not in (1, 2):
        die("usage: msr <hex_addr> [cpu]")
    addr_s = argv[0]
    cpu = argv[1] if len(argv) == 2 else "0"
    if not re.fullmatch(r"(0x)?[0-9a-fA-F]{1,8}", addr_s):
        die(f"bad MSR address {addr_s!r}")
    if not re.fullmatch(r"\d{1,3}", cpu):
        die(f"bad cpu index {cpu!r}")
    addr = int(addr_s, 16)
    subprocess.run(["modprobe", "msr"], check=False)
    try:
        fd = os.open(f"/dev/cpu/{cpu}/msr", os.O_RDONLY)
        try:
            os.lseek(fd, addr, os.SEEK_SET)
            raw = os.read(fd, 8)
        finally:
            os.close(fd)
    except OSError as e:
        die(f"msr read {addr:#x} on cpu{cpu} failed: {e}")
    val = int.from_bytes(raw, "little")
    print(json.dumps({"cpu": int(cpu), "msr": f"{addr:#x}",
                      "value": f"{val:#018x}", "decimal": val}))

def cmd_msr_write(argv):
    if "--unsafe" not in argv:
        die("MSR writes need --unsafe: a wrong value in the wrong MSR hard-hangs\n"
            "        the machine with no logging. Know the register first.")
    argv = [a for a in argv if a != "--unsafe"]
    if len(argv) not in (2, 3):
        die("usage: msr-write <hex_addr> <hex_value> [cpu|all] --unsafe")
    addr_s, val_s = argv[0], argv[1]
    who = argv[2] if len(argv) == 3 else "all"
    if not re.fullmatch(r"(0x)?[0-9a-fA-F]{1,8}", addr_s):
        die(f"bad MSR address {addr_s!r}")
    if not re.fullmatch(r"(0x)?[0-9a-fA-F]{1,16}", val_s):
        die(f"bad MSR value {val_s!r}")
    addr, val = int(addr_s, 16), int(val_s, 16)
    subprocess.run(["modprobe", "msr"], check=False)
    if who == "all":
        cpus = sorted(int(m.group(1)) for d in os.listdir("/dev/cpu")
                      if (m := re.fullmatch(r"(\d+)", d)))
    else:
        if not re.fullmatch(r"\d{1,3}", who):
            die("cpu must be a number or 'all'")
        cpus = [int(who)]
    results = []
    for cpu in cpus:
        try:
            fd = os.open(f"/dev/cpu/{cpu}/msr", os.O_WRONLY)
            try:
                os.lseek(fd, addr, os.SEEK_SET)
                os.write(fd, val.to_bytes(8, "little"))
                results.append({"cpu": cpu, "ok": True})
            finally:
                os.close(fd)
        except OSError as e:
            results.append({"cpu": cpu, "ok": False, "error": str(e)})
    print(json.dumps({"msr": f"{addr:#x}", "value": f"{val:#x}", "results": results}))

def cmd_acpidump(argv):
    outdir = "/var/lib/83sc-control/tables"
    os.makedirs(outdir, mode=0o755, exist_ok=True)
    src = "/sys/firmware/acpi/tables"
    if not os.path.isdir(src):
        die(f"{src} not available")
    written = []
    for entry in sorted(os.listdir(src)):
        path = os.path.join(src, entry)
        if not os.path.isfile(path):
            continue
        try:
            with open(path, "rb") as f:
                data = f.read()
        except OSError as e:
            written.append({"table": entry, "error": str(e)})
            continue
        safe = re.sub(r"[^A-Za-z0-9._-]", "_", entry)
        dest = os.path.join(outdir, safe + ".dat")
        with open(dest, "wb") as f:
            f.write(data)
        os.chmod(dest, 0o644)
        written.append({"table": entry, "bytes": len(data), "file": dest})
    print(json.dumps({"outdir": outdir, "count": len(written), "tables": written}, indent=2))

def cmd_gpu_hotspot(argv):
    HOTSPOT_OFF = 0xAD0AA0
    dev = argv[0] if argv else None
    if dev and not re.fullmatch(r"[0-9a-fA-F]{4}:[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\.\d", dev):
        die("device must be a PCI address like 0000:01:00.0")
    if not dev:
        base = "/sys/bus/pci/devices"
        for d in sorted(os.listdir(base)):
            vendor = read(f"{base}/{d}/vendor")
            cls = read(f"{base}/{d}/class") or ""
            if vendor == "0x10de" and cls.startswith(("0x0300", "0x0302")):
                dev = d
                break
    if not dev:
        die("no NVIDIA GPU found")

    path = f"/sys/bus/pci/devices/{dev}/resource0"
    if not os.path.exists(path):
        die(f"{path} not present")

    import mmap
    page = os.sysconf("SC_PAGE_SIZE")
    base_off = (HOTSPOT_OFF // page) * page
    delta = HOTSPOT_OFF - base_off
    try:
        fd = os.open(path, os.O_RDONLY)
        try:
            size = os.fstat(fd).st_size
            if HOTSPOT_OFF + 4 > size:
                die(f"offset {HOTSPOT_OFF:#x} beyond BAR0 size {size:#x}")
            mm = mmap.mmap(fd, page * 2, mmap.MAP_SHARED, mmap.PROT_READ, offset=base_off)
            try:
                raw = int.from_bytes(mm[delta:delta + 4], "little")
            finally:
                mm.close()
        finally:
            os.close(fd)
    except OSError as e:
        die(f"BAR0 read failed: {e}")

    celsius = (raw & 0xFFFF) / 256.0
    out = {"device": dev, "offset": f"{HOTSPOT_OFF:#x}", "raw": f"{raw:#010x}",
           "hotspot_c": round(celsius, 2)}
    out["plausible"] = 15.0 <= celsius <= 125.0
    print(json.dumps(out))

UNDERVOLT_CONF = "/etc/intel-undervolt.conf"


def cmd_undervolt(argv):
    """Apply a core+cache undervolt offset in millivolts.

    Core and cache share a voltage rail on this part, so offsetting core alone
    leaves the cache domain holding the floor and nothing moves. They are always
    written together.

    Measured on the 83SC at PL1 55 W: -100 mV gave +302 MHz, 6 C cooler, 4 W
    less, because the chip is thermally limited rather than power limited.
    """
    if not argv:
        die("usage: undervolt <mv>   (negative = undervolt, positive = overvolt, 0 = off)")
    raw = argv[0]
    if not re.fullmatch(r"[+-]?\d{1,3}", raw):
        die("offset must be a signed millivolt value, e.g. -100 or +25")
    off = int(raw)
    # Undervolting is bounded by stability; overvolting is bounded by damage, so
    # the positive side is capped far tighter.
    if off < -250:
        die("refusing undervolt beyond -250 mV")
    if off > 50:
        die("refusing overvolt beyond +50 mV - raising core voltage accelerates "
            "electromigration and this chip has CPU overclocking disabled in "
            "firmware, so there is no clock headroom for it to buy")
    mv = off
    if not os.path.exists(UNDERVOLT_CONF):
        die(f"{UNDERVOLT_CONF} not found (install intel-undervolt)")
    exe = "/usr/bin/intel-undervolt"
    if not os.path.exists(exe):
        die("intel-undervolt not installed")

    with open(UNDERVOLT_CONF) as f:
        conf = f.read()
    conf = re.sub(r"^enable .*$", "enable yes" if mv else "enable no", conf, flags=re.M)
    conf = re.sub(r"^undervolt 0 'CPU' .*$", f"undervolt 0 'CPU' {mv}", conf, flags=re.M)
    conf = re.sub(r"^undervolt 2 'CPU Cache' .*$", f"undervolt 2 'CPU Cache' {mv}", conf, flags=re.M)
    with open(UNDERVOLT_CONF, "w") as f:
        f.write(conf)

    r = subprocess.run([exe, "apply"], capture_output=True, text=True)
    rb = subprocess.run([exe, "read"], capture_output=True, text=True)
    applied = None
    m = re.search(r"^CPU \(0\): *(-?[\d.]+) mV", rb.stdout or "", re.M)
    if m:
        applied = float(m.group(1))
    print(json.dumps({"requested_mv": mv, "applied_mv": applied,
                      "rc": r.returncode, "output": (r.stdout or "").strip()[:300]}))
    # The mailbox silently ignores writes when UnderVolt Protection is enabled in
    # BIOS, so verify rather than trust the return code.
    if mv and (applied is None or abs(applied) < abs(mv) * 0.8):
        print("helper: WARNING offset did not take - check UnderVolt Protection in BIOS",
              file=sys.stderr)
        return 4
    return 0


BOOT_CONF = "/etc/83sc-control/boot.conf"


def cmd_boot_save(argv):
    """Snapshot the live hardware state to /etc/83sc-control/boot.conf.

    83sc-thermal.service replays this at boot. Written from what the hardware
    actually reports rather than from what a caller claims, so it can never
    record a setting that did not take.
    """
    lh = hwmon("legion_hwmon")
    lines = ["# written by 83sc-control boot-save", "# replayed by 83sc-thermal.service"]

    def put(k, v):
        if v is not None and v != "":
            lines.append(f"{k}={v}")

    put("POWERMODE", read(f"{LEGION}/powermode"))
    put("FAN_FULLSPEED", read(f"{LEGION}/fan_fullspeed"))
    put("GPU_CTGP", read(f"{LEGION}/gpu_ctgp_powerlimit"))
    put("GPU_PPAB", read(f"{LEGION}/gpu_ppab_powerlimit"))
    for key, node in (("PL1_UW", "constraint_0_power_limit_uw"),
                      ("PL2_UW", "constraint_1_power_limit_uw"),
                      ("TAU_US", "constraint_0_time_window_us")):
        put(key, read(f"{RAPL}/{node}"))
    put("MAX_PERF_PCT", read(f"{PSTATE}/max_perf_pct"))

    uv = 0
    try:
        r = subprocess.run(["/usr/bin/intel-undervolt", "read"], capture_output=True, text=True)
        m = re.search(r"^CPU \(0\): *(-?[\d.]+) mV", r.stdout or "", re.M)
        if m:
            uv = int(round(float(m.group(1))))
    except OSError:
        pass
    put("UNDERVOLT_MV", uv)

    if lh:
        curve = []
        for i in range(1, 11):
            t = read(f"{lh}/pwm1_auto_point{i}_temp")
            p = read(f"{lh}/pwm1_auto_point{i}_pwm")
            if t is None or p is None:
                curve = []
                break
            curve.append(f"{t}:{p}")
        if curve:
            put("CURVE_PWM", ",".join(curve))
            put("FAN_MAX", read(f"{lh}/fan1_max"))

    os.makedirs(os.path.dirname(BOOT_CONF), mode=0o755, exist_ok=True)
    with open(BOOT_CONF, "w") as f:
        f.write("\n".join(lines) + "\n")
    os.chmod(BOOT_CONF, 0o644)
    print(json.dumps({"file": BOOT_CONF, "settings": len(lines) - 2}))


def cmd_undervolt_persist(argv):
    """Enable or disable the undervolt surviving reboot.

    Uses intel-undervolt's own systemd unit rather than a bespoke mechanism, so
    the offset written by 'undervolt' is what gets re-applied at boot.
    """
    if not argv or argv[0] not in ("0", "1"):
        die("usage: undervolt-persist <0|1>")
    on = argv[0] == "1"
    unit = "intel-undervolt.service"
    action = "enable" if on else "disable"
    r = subprocess.run(["systemctl", action, unit], capture_output=True, text=True)
    en = subprocess.run(["systemctl", "is-enabled", unit], capture_output=True, text=True)
    print(json.dumps({"unit": unit, "action": action, "rc": r.returncode,
                      "is_enabled": en.stdout.strip(),
                      "error": r.stderr.strip()[:200]}))
    return 0 if r.returncode == 0 else 3


def cmd_dump_legion(argv):
    allowed = {"ecmemory", "ecmemoryram", "fancurve"}
    name = argv[0] if argv else "ecmemory"
    if name not in allowed:
        die(f"file must be one of: {', '.join(sorted(allowed))}")
    path = f"/sys/kernel/debug/legion/{name}"
    if not os.path.exists(path):
        die(f"{path} not present (is legion_laptop loaded?)")
    try:
        with open(path, "rb") as f:
            data = f.read()
    except OSError as e:
        die(f"read failed: {e}")
    if name == "fancurve":
        print(json.dumps({"file": name, "text": data.decode(errors="replace")}))
    else:
        print(json.dumps({"file": name, "size": len(data), "hex": data.hex()}))


def cmd_modprobe(argv):
    if not argv:
        die("usage: modprobe <module> [args...]")
    mod = argv[0]
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", mod):
        die("bad module name")
    extra = []
    for a in argv[1:]:
        if not re.fullmatch(r"[A-Za-z0-9_]+=[A-Za-z0-9_,.-]+", a):
            die(f"bad module parameter {a!r}")
        extra.append(a)
    r = subprocess.run(["modprobe", mod, *extra], capture_output=True, text=True)
    print(json.dumps({"module": mod, "params": extra, "rc": r.returncode,
                      "stdout": r.stdout.strip(), "stderr": r.stderr.strip()}))
    return r.returncode

USAGE = """83sc-control helper (runs as root)

  status                     full machine state as JSON
  targets                    named shortcuts + writable path prefixes
  set <name|path> <value>    write any hardware knob, verified by readback
  fan-curve <pt> <f> <v>     write a fan curve point        [--unsafe]
  msr <hex> [cpu]            read an MSR
  msr-write <hex> <val>      write an MSR                   [--unsafe]
  dump-ec                    raw EC register space as hex
  ec-write <off> <byte>      raw EC register write          [--unsafe]
  gpu-hotspot [pci_addr]     Blackwell junction temp via BAR0 (read-only)
  acpidump [filename]        dump ACPI tables to /var/lib/83sc-control/
  modprobe <mod> [k=v...]    load a kernel module

Writes are confined to hardware subsystems (see 'targets'). --unsafe gates
operations that can hang the machine or confuse the EC; it stops accidents,
not attackers.
"""

def main():
    if os.geteuid() != 0:
        die("must run as root (invoke via the 83sc CLI)")
    if len(sys.argv) < 2:
        print(USAGE)
        return 1
    verb, rest = sys.argv[1], sys.argv[2:]
    dispatch = {
        "status": lambda: cmd_status(), "targets": lambda: cmd_targets(),
        "set": lambda: cmd_set(rest), "fan-curve": lambda: cmd_fancurve(rest),
        "msr": lambda: cmd_msr(rest), "msr-write": lambda: cmd_msr_write(rest),
        "dump-ec": lambda: cmd_dump_ec(), "ec-write": lambda: cmd_ec_write(rest),
        "dump-legion": lambda: cmd_dump_legion(rest),
        "undervolt": lambda: cmd_undervolt(rest),
        "undervolt-persist": lambda: cmd_undervolt_persist(rest),
        "boot-save": lambda: cmd_boot_save(rest),
        "gpu-hotspot": lambda: cmd_gpu_hotspot(rest),
        "acpidump": lambda: cmd_acpidump(rest), "modprobe": lambda: cmd_modprobe(rest),
    }
    if verb not in dispatch:
        die(f"unknown verb {verb!r}\n\n{USAGE}")
    return dispatch[verb]()

if __name__ == "__main__":
    sys.exit(main() or 0)
