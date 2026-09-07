# Lenovo LOQ Essential 15IRX11 (83SC) — thermal/stutter investigation

**Machine:** DMI `83SC`, BIOS `SECN14WW` (2025-06-13), EC chip `0x5508`
**CPU:** i7-13650HX (6P+8E) · **GPU:** RTX 5050 Laptop · **OS:** CachyOS, kernel 6.18.48-1-cachyos-lts

---

## Root cause

**A 55W sustained CPU power limit on a single-fan cooler.**

Everything else follows from that one mismatch.

| | stock | corrected |
|---|---|---|
| PL1 sustained | 55 W | 45 W |
| PL2 burst | 130 W | 90 W |
| PL1 window (tau) | 56 s | 8 s |
| Fan | ~2600 rpm auto | 4300 rpm pinned |
| Result | 97 °C, chronic throttling | 70–72 °C, none |

Validated: zero PROCHOT events across 240 s of combined CPU+GPU load
(CPU 81 °C peak / 73 °C avg, GPU sustained 40 W).

---

## The stutter mechanism

The reported "400 MHz clock crashes" were never a P-state.

- `MSR_PLATFORM_INFO` minimum efficiency ratio = **800 MHz**. The silicon cannot be *commanded* below it.
- `MSR_TEMPERATURE_TARGET`: Tjmax 100 °C, TCC offset 3 °C → **throttles at 97 °C**.
- At TCC the CPU holds its 800 MHz P-state and begins **duty-cycling (T-states)**, halving throughput. MangoHud samples effective clock and reports ~400 MHz.

This is why every governor and `cpupower` experiment was inconclusive: the damage happens in T-states, a layer beneath cpufreq. `scaling_min_freq` is 800 MHz, so no governor setting can reach it.

`MSR_CORE_PERF_LIMIT_REASONS` (0x64F) sticky logs confirmed **PROCHOT + Thermal + PL1** had all fired.

**`MSR_PKG_POWER_LIMIT` (0x610) bit 63 = 0 — the BIOS leaves power limits unlocked.** No firmware flash is needed for any of this.

---

## The single-fan discovery

The laptop has **one physical fan**. Every tool reports two.

Evidence: 60 samples across a full thermal ramp with auto fan control, RPM sweeping through ~15 distinct values (3000 → 2000 → 3400). `fan1 == fan2` at **every** sample — 0/60 divergence. Independent fans drift by ~100 rpm from bearing wear and airflow tolerance alone; perfect lock-step across 15 values means one tachometer read twice.

Firmware ships **15 fan tables** split across `fanid=1` (CPU, sensor 4) and `fanid=2` (GPU, sensor 5) — a real two-fan design shared across the chassis family. The Legion siblings have both fans; the LOQ Essential is the cost-reduced single-fan variant. The EC's 55W PL1 was sized for airflow this machine does not have.

**Consequence:** thermal paste cannot fix this. Fresh paste is worth 3–5 °C against a ~25 °C structural gap.

### Heatpipe topology (physical, owner-verified)

```
CPU block (centre) → small chip block → GPU block (large) → FAN
```

CPU heat traverses the entire pipe; the GPU sits last, closest to the fan, and stays cool (~52 °C idle, ~77 °C loaded). The GPU block is physically larger than the CPU block despite the CPU dissipating far more — the cooler is mis-proportioned for the actual load.

---

## Factory fan curves (extracted from ACPI)

Performance mode, CPU sensor:

```
temp °C :  75    78    80    85    85    85    85    85    97   100
fan rpm :   0  1800  2300  3000  3300  3500  4000  4300  4300  4300
```

**All 15 tables cap at 4300 rpm**, reached at 85 °C. From 85 → 97 °C the fan is already maxed. hwmon's `fan1_max = 10000` is a sysfs convention, not the hardware ceiling. `FTTD` contains `5400`, a higher ceiling not reachable through any tested path.

Ramp rates differ sharply by mode — at 80 °C: mode 3 (perf) = 2300 rpm, mode 224 (extreme) = 4000 rpm.

---

## Control paths that work

| Goal | Working path | Broken path |
|---|---|---|
| Custom mode | `powermode` = **255** | `platform_profile` = `custom` → EINVAL *(correct: `custom` is read-only status per kernel ABI, not a bug)* |
| Extreme mode | `powermode` = **224** | — |
| Power limits | **RAPL** `constraint_*_power_limit_uw` | `ppt_pl1_spl` / `ppt_pl2_sppt` → EINVAL |
| Max fan | `fan_fullspeed` = 1 **in powermode 255** | `fan_fullspeed` in mode 3 → silent no-op |

`fan_fullspeed` verified: 4300 rpm on vs 3700 off at identical temperature. `fan_unlock` alone had no measurable effect.

### ACPI fan structures

- **`FAT2`** (88 B) — what LLL implements via `GFAN`. `FTS0-9` and `FSS0-9` are *duplicate* copies of the same speed array. **No temperature data.**
- **`FACT`** (72 B) — `FNS0-9` (speeds), `SEID`, **`SST0-9` (trip points)**, plus `SOU1-4`, `CFMS`, `CFIS`, `FSSP`, `MST1/2`, `MSTP`. Populated by `SFTW()` from the `FNT0`/`FNT1` packages (15 tables × 35 elements) — **firmware-internal, not reachable via WMI.**

**WMI dispatch (resolved).** The fan GUID `92549549-4BDE-4F06-AC04-CE8BF898DBAA` maps to ACPI method `WMAB`, which handles exactly two ids:

```
Arg1 == 0x05  ->  GFAN (FID0, SID0)     get  (returns FAT2, speeds only)
Arg1 == 0x06  ->  SFAN (Arg2)           set
```

`SFAN` declares ten temperature words (`F00F`–`F018`), a sensor id (`F00D`) and sensor length (`F00E`) — and **never reads any of them**; each appears exactly once across all 238 lines, in its own declaration. Thresholds come from internal packages `FI00`–`FI09` chosen by power mode. Further, `F003`–`F00C` are **indices into a preset speed table**, not RPM: `CRP0 = Local0[F003 + 2] / 100`.

**Conclusion:** fan-curve temperatures cannot be written on this model by any caller, Windows included. Reading them works via the EC (`ACCESS_METHOD_EC3`), which is the fix applied.

---

## Validation status

| Scenario | Result |
|---|---|
| CPU-only, 14 threads, 90 s | **70–72 °C, zero throttling** after tau engages |
| tau 56 s → 8 s | initial-spike events **1176 → 91** (−92%) |
| CPU + GPU combined, 240 s | **0 PROCHOT events**, CPU 81 °C peak / 73 °C avg, GPU 40 W sustained |
| Fan curve after `EC3` fix | real trip temps + rpm, matching firmware `FNT0` exactly |
| `fan_fullspeed` guard | `EBUSY` in powermode 3, succeeds in 255 |
| Real CS2 match | **Not yet run** |

Note: an earlier combined run measured nothing because `stress-ng --gpu` never
engaged the dGPU (3.5 W = idle, no display context). The 240 s result above used
a CUDA load reaching a real 40 W.

---

## Resolved since first draft

- **Fan curve** — `FACT` is unreachable via WMI, and `SFAN` discards the
  temperature fields it declares. The real fix was `model_secn`'s
  `access_method_fancurve`: `WMI3` → `EC3`. Temps and RPM now read correctly.
- **GPU throttling** — the hotspot sensor is not needed. NVML throttle-reason
  bits work on driver 610.57.04 and are a direct signal. Direct BAR0 reads at
  the LACT offset (`0xad0aa0`) are blocked by `CONFIG_IO_STRICT_DEVMEM` while
  nvidia holds the region; that belongs in a kernel module, not userspace.
- **Combined load** — validated, zero PROCHOT.
- **Persistence** — `systemd/83sc-thermal.{sh,service}`, with runtime profiles.

## Open items

1. Reach the 5400 rpm ceiling declared in `FTTD` (observed rarely under Windows).
2. Per-model pwm↔rpm scaling — `MAX_RPM` 10000 skews reported duty by ~2×.
3. Tune PL1 between 45 W and 55 W; 45 W is proven, not optimised.
