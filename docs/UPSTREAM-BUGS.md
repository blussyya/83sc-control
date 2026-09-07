# Upstream bug reports — LenovoLegionLinux / lenovo-wmi-other

**Target:** https://github.com/johnfanv2/LenovoLegionLinux/issues

Copy each section as its own issue. All are reproducible with a single command.

---

## System information (include in every report)

```
Model:        Lenovo LOQ Essential 15IRX11
DMI product:  83SC
BIOS:         SECN14WW (2025-06-13)
Board:        LNVNB161216
EC chip ID:   0x5508
CPU:          Intel i7-13650HX (6P+8E)
GPU:          NVIDIA RTX 5050 Laptop
Kernel:       6.18.48-1-cachyos-lts
Driver:       legion_laptop, LenovoLegionLinux 0.0.22.r0.g71995ce
Matched:      model_secn (already in the allowlist — force=1 not required)
```

---

## Bug 1 — hwmon exposes a phantom second fan

**Severity: high** (fabricated hardware shown to users)

This machine has **one physical fan** (owner-confirmed, chassis opened multiple times). The driver exposes `fan1_input` and `fan2_input`.

**Reproduce** — sample both during a thermal ramp with auto fan control:

```bash
H=/sys/class/hwmon/hwmon5   # the legion_hwmon node
for i in $(seq 1 60); do echo "$(cat $H/fan1_input)/$(cat $H/fan2_input)"; sleep 1; done
```

**Result:** identical at all 60 samples across ~15 distinct RPM values (3000 → 2000 → 3400). 0/60 divergence. Two independent fans drift by ~100 rpm from bearing/airflow tolerance; perfect lock-step means one tachometer read twice.

**Cause:** firmware ships 15 fan tables for `fanid=1` (CPU sensor 4) and `fanid=2` (GPU sensor 5) — a genuine two-fan design shared across the chassis family. The LOQ Essential is the single-fan cost-reduced variant. The driver trusts the table count rather than the hardware.

**Suggested fix:** gate `fan2_*` behind a per-model `has_second_fan` flag in `model_config`, defaulting false for `model_secn`.

---

## Bug 2 — `platform_profile` advertises `custom` then rejects it

**Severity: high** (blocks all custom-mode functionality)

```bash
$ cat /sys/firmware/acpi/platform_profile_choices
low-power balanced performance custom

$ echo custom | sudo tee /sys/firmware/acpi/platform_profile
tee: ...: Invalid argument
```

Writing the raw EC powermode **works** and `platform_profile` then reads back `custom`:

```bash
$ echo 255 | sudo tee /sys/bus/platform/devices/PNP0C09:00/powermode
$ cat /sys/firmware/acpi/platform_profile
custom
```

The profile→powermode mapping omits `custom` = `255` while still listing it in `_choices`. This is the gate on Bug 3 and on power-limit control.

---

## Bug 3 — `fan_fullspeed` silently no-ops outside custom powermode

**Severity: high** (write reports success, does nothing)

In `powermode=3` (performance):

```bash
$ echo 1 | sudo tee /sys/bus/platform/devices/PNP0C09:00/fan_fullspeed
$ cat /sys/bus/platform/devices/PNP0C09:00/fan_fullspeed
1
```

Readback is `1`; fan speed does not change (2600 rpm before and after, 20 s settle).

In `powermode=255` the same write pins the fan to **4300 rpm** — verified against **3700 rpm** with `fan_fullspeed=0` at identical temperature.

**Suggested fix:** either return `-EBUSY`/`-EINVAL` when the mode cannot honour it, or transition the powermode as part of the write. Silent success is the worst option — it sent this user down a long dead end.

---

## Bug 4 — fan curve uses the speed-only structure, discarding all temperatures

**Severity: high** (feature works on Windows, absent on Linux)

Every `pwmN_auto_pointM_temp` reads `0`, and `pwm2_auto_point*_pwm` is all zeros, making the curve look corrupt.

**Cause:** `wmi_read_fancurve_custom()` reads `FAT2` via `GFAN`:

```c
struct wmi_fan_table_read {
    __le32 fan_table_length;
    __le32 fan_speed[MAXFANCURVESIZE];
};
```

In firmware, `FAT2` (88 B) holds `FTS0-9` and `FSS0-9` — **duplicate copies of the same speed array**, no temperatures. So zeros are accurate; there is nothing to read.

The real structure is **`FACT`** (72 B), populated by `SFTW()`:

```
0x00 FNIM  0x02 FNID  0x04 FNLE
0x08..0x1A FNS0-FNS9   10 fan speeds (16-bit RPM)
0x1C SEID              sensor id
0x20 STLE              sensor table length
0x24..0x36 SST0-SST9   10 temperature trip points   <-- missing today
0x38 SOU1  0x39 SOU2  0x3A CFMS  0x3C SOU3  0x3D SOU4
0x3E CFIS  0x40 FSSP  0x42 MST1  0x44 MST2  0x46 MSTP
```

`FNT0`/`FNT1` contain 15 factory tables (35 elements each). Extracted example — performance mode, CPU sensor:

```
temp °C :  75    78    80    85    85    85    85    85    97   100
fan rpm :   0  1800  2300  3000  3300  3500  4000  4300  4300  4300
```

Values confirmed by the owner against Lenovo Legion Toolkit on Windows.

**Suggested fix:** add a `FACT`-based access method that populates `tmp_cpu`/`tmp_gpu` from `SST0-9` and speeds from `FNS0-9`. Existing `FAT2` paths stay untouched for models that need them.

---

## Bug 5 — `ppt_pl1_spl` / `ppt_pl2_sppt` return EINVAL in every mode

**Severity: medium** (driver: `lenovo-wmi-other`)

```bash
$ cat /sys/class/firmware-attributes/lenovo-wmi-other-0/attributes/ppt_pl1_spl/current_value
cat: ...: Invalid argument
```

`default_value` (45), `min_value` (25) and `max_value` (60) all read correctly; only `current_value` fails, for read **and** write, in `performance`, `balanced` and `custom` (powermode 255). `ppt_pl3_fppt` fails on every attribute.

Workaround: Intel RAPL (`/sys/class/powercap/intel-rapl:0/`) works and is authoritative. `MSR_PKG_POWER_LIMIT` bit 63 = 0, so the BIOS does not lock power limits on this model.

---

## Bug 6 — `platform_profile` reads back `None` in powermode 224

**Severity: low**

After `echo 224 > powermode` (extreme), reading `platform_profile` yields no value, breaking consumers that assume a valid string. Either map 224 to a profile name or reject the write.

---

## Bug 7 — `fan1_max` reports 10000 rpm; real ceiling is 4300

**Severity: low**

hwmon advertises `fan1_max = 10000`. All 15 firmware tables cap at **4300 rpm**, which is also the maximum observed with `fan_fullspeed=1`. (`FTTD` contains `5400`, not reachable through any tested path.) Userspace fan-percentage displays are wrong by >2×.

---

## Note for maintainers

`model_secn` is already in the allowlist and matches correctly — `force=1` is **not** required on this model, contrary to several community guides.

The underlying hardware issue this exposed: stock PL1 is **55 W sustained on a single-fan cooler**, driving the CPU into TCC at 97 °C and T-state duty-cycling. Reducing PL1 to 40 W and tau from 56 s to 8 s holds 70–72 °C with zero throttling under a 14-thread all-core load. A per-model sane-default power limit for single-fan variants would fix this for every owner.
