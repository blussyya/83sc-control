# LenovoLegionLinux — findings for LOQ Essential 15IRX11 (83SC)

Reference material. Every claim below was reproduced on real hardware; commands
are included so a maintainer can verify independently.

```
model:  LOQ Essential 15IRX11, DMI 83SC, BIOS SECN14WW, EC 0x5508
specs:  i7-13650HX (6P+8E, 14C/20T), RTX 5050 8GB, 16GB DDR5
kernel: 6.18.48  ·  driver: legion_laptop (LenovoLegionLinux 0.0.22)
match:  model_secn  ·  force=1 NOT required, the DMI match works
```

The 83SC ships in several configurations (i5-13450HX or i7-13650HX, RTX 5050 or
5060, both GPUs at 65 W). Measurements below are from the i7/5050 build.

---

## 1. Fan curve reads all zeros — `model_secn` uses the wrong access method

**Highest impact. One-line fix. Likely affects every 83SC owner.**

`model_secn` sets `access_method_fancurve = ACCESS_METHOD_WMI3`. That path reads
`FAT2` via the firmware's `GFAN` method, and `FAT2` carries **no temperature
data at all** — `FTS0-9` and `FSS0-9` are duplicate copies of the same speed
array. So every `pwmN_auto_pointM_temp` reads `0`, and the curve looks corrupt.

But `model_secn` already declares `.registers = &ec_register_offsets_loq_v1` —
the same table `model_r3cn` (LOQ 15IRX10, same EC 0x5508) uses with
`ACCESS_METHOD_EC3` / `ec_read_fancurve_loq`, which does populate temperatures.

**Fix:** `ACCESS_METHOD_WMI3` → `ACCESS_METHOD_EC3` for `model_secn`.

Before:

```
pt1 pwm=2  cpu=0   pt2 pwm=5  cpu=0   pt3 pwm=7  cpu=0
```

After:

```
pt1 pwm=0   cpu=75    pt2 pwm=86  cpu=78
pt3 pwm=102 cpu=80    pt4 pwm=109 cpu=85
```

Decoded that is `0 / 3400 / 4000 / 4300 rpm` at `75 / 78 / 80 / 85 °C` — an
exact match for the `FNT0` mode-224 table extracted from the machine's own ACPI
tables. Raw EC bytes confirm the `(min_temp, max_temp, rpm÷100)` triples:

```
+107: 00 4b 00   ->  --  75C     0 rpm
+10a: 3c 4e 22   ->  60  78C  3400 rpm
+10d: 40 50 28   ->  64  80C  4000 rpm
+110: 43 55 2b   ->  67  85C  4300 rpm
+12b: 4b 61 2b   ->  75  97C  4300 rpm
+12e: 5e 64 2b   ->  94 100C  4300 rpm
```

**Caveat worth reviewing:** this is one machine. If `WMI3` was chosen
deliberately for some 83SC variant, that context would be valuable.

---

## 2. `fan_fullspeed` silently no-ops outside custom powermode

Write reports success and readback confirms the new value, but fan speed does
not change.

```bash
# powermode 3 (performance)
echo 1 | sudo tee /sys/bus/platform/devices/PNP0C09:00/fan_fullspeed   # -> 1
# fan stays at ~2600 rpm, 20s settle
```

In `powermode 255` (custom) the same write pins the fan to **4300 rpm**,
verified against **3700 rpm** with it off at identical temperature.

The firmware accepts the WMI call and stores the flag; the fan controller
ignores it outside custom mode. Readback cannot detect this — the flag really is
set — so only a powermode guard works.

**Fix:** per-model `fanfullspeed_requires_custom_powermode`; return `-EBUSY`
rather than reporting success. Verified: `EBUSY` in powermode 3, succeeds in 255.

---

## 3. hwmon exposes a phantom `fan2`

This chassis has **one physical fan** (owner-confirmed, opened repeatedly). The
driver exposes `fan2_input`, `fan2_label`, `fan2_target`, `fan2_max`.

```bash
H=/sys/class/hwmon/hwmon5
for i in $(seq 1 60); do echo "$(cat $H/fan1_input)/$(cat $H/fan2_input)"; sleep 1; done
```

Identical at all 60 samples across ~15 distinct RPM values during a thermal ramp
(3000 → 2000 → 3400). **0/60 divergence.** Independent fans drift ~100 rpm from
bearing and airflow tolerance; perfect lock-step means one tachometer read twice.

Firmware ships 15 fan tables split across `fanid=1` (CPU, sensor 4) and
`fanid=2` (GPU, sensor 5) — a real two-fan design shared across the chassis
family. The LOQ Essential is the single-fan variant.

**Fix:** per-model `has_single_fan`, gating the fan2 *tachometer* attributes in
the hwmon `is_visible` callbacks. Follows the existing `has_four_fans` pattern.

**Deliberately not hidden:** `pwm2_auto_point*`. Those are the GPU-sensor curve,
not fan-2 telemetry. They read zero here because the `fanid=2` curve drives a
fan that does not exist — EC memory past `+0x133` is entirely zero, with no
60-75 °C pattern at any stride 1-8. GPU *monitoring* is unaffected;
`temp2_input` reports 38-52 °C normally.

---

## 4. `fan1_max` reports 10000 rpm; real ceiling is 5400

hwmon returns the global `MAX_RPM` (10000). All 15 firmware tables cap at
**4300 rpm**, and `FTTD` declares **5400** as the hardware maximum (observed
rarely under Windows). Userspace fan-percentage displays are wrong by roughly 2×.

**Fix:** per-model `fan_max_rpm` (5400 for `model_secn`), falling back to
`MAX_RPM`. Verified reading 5400.

**Related, not patched:** `MAX_RPM` also drives the `FAN_SPEED_UNIT_RPM_HUNDRED`
pwm↔rpm conversion, so reported pwm is ~43% where the fan is at ~80% of its real
ceiling. Fixing that properly means threading a per-model max through
`struct fancurve`, which touches shared paths for every model — left for
maintainer judgement. Note the `migrate-to-pwm` branch (Oct 2024, unmerged)
attempted a related rework.

---

## 5. `platform_profile` read fails in powermode 224 on kernels < 6.19

`legion_platform_profile_get` handles `LEGION_WMI_POWERMODE_MAX_POWER` only
under `#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 19, 0)`, because
`PLATFORM_PROFILE_MAX_POWER` does not exist before then. On older kernels
powermode 224 falls to `default: return -EINVAL`, so reads fail outright.

```bash
echo 224 | sudo tee /sys/bus/platform/devices/PNP0C09:00/powermode
cat /sys/firmware/acpi/platform_profile     # returns nothing
```

**Fix:** map 224 to `PLATFORM_PROFILE_PERFORMANCE` on kernels below 6.19.
Kernels 6.19+ are unaffected and still map to `MAX_POWER`. Verified: now reads
`performance` on 6.18.

---

## 6. `cpu_longterm_powerlimit` writes are stored but never applied

**Reported without a patch** — the driver cannot detect this, and hiding a
control on one negative result seemed worse than documenting it.

```bash
cat /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw   # 45000000
echo 30 | sudo tee /sys/bus/platform/devices/PNP0C09:00/cpu_longterm_powerlimit
cat /sys/bus/platform/devices/PNP0C09:00/cpu_longterm_powerlimit   # 30  (stored)
cat /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw   # 45000000 (unchanged)
```

The value persists across a full powermode toggle (255 → 3 → 255) and RAPL never
follows. `MSR_PKG_POWER_LIMIT` (0x610) confirms no change. No commit trigger was
found. Whether this WMI feature does something else on this model is unknown.

---

## Not bugs — checked and withdrawn

**`platform_profile` rejecting `custom` is correct.** Writing `custom` returns
`EINVAL` even when already in custom mode. Per the kernel's own documentation,
`custom` is *"intended to be set by drivers when the settings in the driver have
been modified in a way that a standard profile doesn't represent the current
state"* — it is driver-reported status, not user-settable. Writing `255` to the
EC `powermode` attribute is the correct route, after which `platform_profile`
correctly reports `custom`.

**Fan-curve temperatures cannot be written on this model.** `SFAN` declares ten
temperature words (`F00F`–`F018`, offsets `0x1F`–`0x31`), the sensor id
(`F00D`) and sensor table length (`F00E`) — and across all 238 lines of the
method each appears exactly once, in its own `CreateWordField` declaration, and
is never read. Thresholds come from firmware-internal packages `FI00`–`FI09`
selected by power mode. Additionally `F003`–`F00C` are **indices into a preset
speed table**, not RPM: `CRP0 = Local0[F003 + 2] / 100`. So a patch marshalling
temperatures into the write buffer would compile, lint, and do nothing.

**`ppt_pl1_spl` / `ppt_pl2_sppt` returning EINVAL** belongs to mainline
`lenovo-wmi-other`, not this driver, and has no practical impact — RAPL provides
the same control and works.

---

## Underlying hardware issue (context, not a driver bug)

Stock firmware sets **PL1 = 55 W sustained** on a single-fan cooler whose fan
maxes at 4300 rpm by 85 °C. The CPU reaches TCC at **97 °C** (Tjmax 100, offset
3) and duty-cycles via T-states. Userspace samples that as ~400 MHz — *below*
`scaling_min_freq` (800 MHz) — so it presents as a cpufreq/governor bug and is
not one. No governor setting can prevent it.

Reducing PL1 to 45 W and the PL1 window from 56 s to 8 s holds 70-72 °C with
zero PROCHOT events under a 14-thread all-core load, and 0 events across 240 s of
combined CPU+GPU load. This is done entirely through mainline `intel_rapl`;
`MSR_PKG_POWER_LIMIT` bit 63 is clear, so the BIOS does not lock power limits.

A sane per-model default power limit for single-fan variants would fix this for
every owner of this machine.
