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

## Install

One command, from a fresh clone:

```bash
git clone https://github.com/blussyya/83sc-control.git
cd 83sc-control && ./setup.sh
```

> If you get `Permission denied`, run `bash setup.sh` — and tell me, because it means
> the executable bit is missing from the repo again (this project is developed on an
> exFAT drive, which cannot store it).

Run it as yourself — it re-invokes itself with sudo for the privileged parts and drops
back to your session for the user service. It is idempotent, so re-run it any time
(after a kernel upgrade, say). It installs:

1. the privileged helper and a sudoers rule scoped to exactly that one file
2. the CLI tools (`83sc`, `83sc-diag`, `83sc-fan`, `83sc-snap`)
3. the patched `legion_laptop` via DKMS — skipped if your kernel already ships
   `legion-laptop` ≥ `v0.0.26`, which carries these fixes upstream
4. `83sc-thermal.service` (replays power limits, undervolt and fan curve at boot) and
   `83sc-driver-guard.service` (see below)
5. the GUI — `83SC Control` in your app menu
6. the keyboard idle dimmer (`83sc-kbd-idle`, backlight off after 5 s idle)

It verifies the result at the end and tells you what, if anything, did not apply.

To undo: `./setup.sh --remove` (keeps `/etc/83sc-control` so your profiles survive).

### Why there is a driver guard

The distro package registers its own DKMS module built from a pre-fix commit. Both
produce the same `legion-laptop.ko`, so on a kernel upgrade whichever DKMS installs last
wins — silently. When the stock one wins, the phantom fan2 returns, fan curve writes
fail and the temperature points read back 0. `83sc-driver-guard` detects that by its
symptoms and repairs it, at boot and (on Arch-likes) immediately post-upgrade. It stands
down on its own once the packaged module carries the fixes.

See [docs/PORTABILITY.md](docs/PORTABILITY.md) for kernel upgrades and moving distro.

Edit the values at the top of `83sc-thermal.sh` (plain watts and seconds).

## Contents

| Path | What |
|---|---|
| `systemd/` | boot-time power limits — this is the actual fix |
| `bin/83sc` | CLI: status, live watch, presets, arbitrary knob writes |
| `bin/83sc-diag` | decodes the Intel MSRs that explain throttling |
| `bin/83sc-fan` | fan curve: show, apply a preset, restore firmware defaults |
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
