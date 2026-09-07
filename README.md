# 83sc-control

Thermal and power control tooling for the **Lenovo LOQ Essential 15IRX11** (DMI `83SC`,
i7-13650HX + RTX 5050, BIOS `SECN`).

Development and testing scratchpad, not a polished product. Built while tracking down a
stutter in CS2 that turned out to be a thermal/power-limit problem.

## The problem it solves

Stock firmware sets **PL1 to 55 W sustained** on a chassis with **one fan**. The CPU
climbs until it hits TCC (Tjmax 100 °C, offset 3 °C → 97 °C), then duty-cycles via
T-states. Userspace samples that as ~400 MHz, which is *below* `scaling_min_freq`
(800 MHz) — so it looks like a cpufreq/governor bug and is not one. No governor setting
can prevent it.

Dropping PL1 to 45 W and the PL1 window from 56 s to 8 s holds 70-72 °C with zero
throttling under a 14-thread all-core load, and 0 PROCHOT events across 240 s of
combined CPU+GPU load.

The fix lives in Intel's mainline `intel_rapl` driver, not in any Lenovo driver.

## Persistent fix

```bash
sudo install -Dm755 systemd/83sc-thermal.sh /usr/local/lib/83sc-control/83sc-thermal.sh
sudo install -Dm644 systemd/83sc-thermal.service /etc/systemd/system/83sc-thermal.service
sudo systemctl enable --now 83sc-thermal.service
```

Edit the values at the top of `83sc-thermal.sh` (plain watts and seconds).

## Contents

| Path | What |
|---|---|
| `systemd/` | boot-time power limits — this is the actual fix |
| `bin/83sc` | CLI: status, live watch, presets, arbitrary knob writes |
| `bin/83sc-diag` | decodes the Intel MSRs that explain throttling |
| `bin/83sc-snap` | one-line state snapshot |
| `helper/helper.py` | root helper; writes are confined to hardware subsystems |
| `harness/` | stress + monitor tooling, run logging |
| `docs/FINDINGS.md` | full investigation writeup |

`bin/` and `harness/` need the helper installed via `install.sh`, which adds a scoped
sudoers rule. The systemd unit above does **not** — use that alone if you only want the
fix.

## Related

Driver bugs found during this work (phantom `fan2`, `fan_fullspeed` silently no-oping
outside powermode 255, `platform_profile` advertising `custom` then rejecting it) belong
to [LenovoLegionLinux](https://github.com/johnfanv2/LenovoLegionLinux) and are reported
separately. None of the code here is part of that driver.
