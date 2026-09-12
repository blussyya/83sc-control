# 83sc-control

Thermal, power and fan control for the **Lenovo LOQ Essential 15IRX11** (DMI `83SC`,
i7-13650HX / RTX 5050).

Stock firmware runs PL1 at 55 W sustained on a single-fan chassis. The CPU climbs to
TCC (97 °C), then duty-cycles via T-states — userspace samples that as ~400 MHz, below
`scaling_min_freq`, so it looks like a governor bug and isn't one. In CS2 it showed up
as multi-second stalls.

Dropping PL1 to 45 W with an 8 s window holds 70–72 °C with zero throttling under a
14-thread load, and no PROCHOT events across 240 s of combined CPU+GPU load.

## Install

**Arch / CachyOS**

```bash
cd packaging && makepkg -si
```

**Debian / Ubuntu**

```bash
./packaging/build-deb.sh && sudo apt install ./dist/83sc-control_1.0.0_amd64.deb
```

**Fedora**

```bash
rpmbuild -ta 83sc-control-1.0.0.tar.gz   # spec: packaging/83sc-control.spec
```

**Any distro, from source**

```bash
./setup.sh
```

Run `setup.sh` as your normal user — it elevates for the privileged parts and drops back
to your session for the user service. It is idempotent, so re-run it after a kernel
upgrade. `./setup.sh --remove` undoes everything.

Build every package your machine has tooling for: `./packaging/build-all.sh`.

### After installing

```bash
sudo systemctl enable --now 83sc-driver-guard 83sc-thermal
systemctl --user enable --now 83sc-kbd-idle
```

Set what you want in **83SC Control** (app menu), then `sudo 83sc boot-save` to have it
replay at boot.

## Requirements

- Fan control needs `legion_laptop` with the 83SC fixes — **LenovoLegionLinux ≥ v0.0.26**.
  Older versions report zeroed fan-curve temperatures and reject writes.
- `intel-undervolt` (optional) to apply a voltage offset at boot.
- Power limits work on any kernel; they use mainline `intel_rapl`, not a Lenovo driver.

## What you get

| | |
|---|---|
| `83SC Control` | GUI: fan curve, power limits, undervolt, profiles bound to KDE power plans |
| `83sc-fan` | show/set the fan curve |
| `83sc-diag` | thermal and power-limit diagnostics |
| `83sc-snap` | one-shot state dump |
| `83sc` | profile and boot-state management |
| `83sc-thermal.service` | replays power limits, undervolt and fan curve at boot |
| `83sc-driver-guard.service` | keeps the fixed driver loaded across kernel upgrades |
| `83sc-kbd-idle` | keyboard backlight off after N seconds idle |

Every privileged write goes through one root-owned helper that validates its arguments
against a whitelist. The GUI and CLI never run as root.

### Driver guard

Distro packages may ship a `legion_laptop` built from a pre-fix commit. Both produce the
same `legion-laptop.ko`, so on a kernel upgrade whichever DKMS installs last wins —
silently. When the stock one wins, the phantom `fan2` returns and fan-curve writes fail.
The guard detects that by its symptoms and repairs it, at boot and (on Arch) immediately
post-upgrade. It stands down once the packaged module carries the fixes.

## Upstream

Five kernel fixes from this work are merged in
[LenovoLegionLinux](https://github.com/johnfanv2/LenovoLegionLinux)
([#539](https://github.com/johnfanv2/LenovoLegionLinux/pull/539), shipped in v0.0.26):
fan curve read over EC3 instead of WMI3, `fan_fullspeed` returning `-EBUSY` outside
custom power mode, the phantom second fan hidden on this single-fan chassis, per-model
`fan_max_rpm`, and `platform_profile` no longer failing in power mode 224 on kernels
< 6.19. Write-up in
[#538](https://github.com/johnfanv2/LenovoLegionLinux/issues/538).

## Docs

- [docs/PORTABILITY.md](docs/PORTABILITY.md) — kernel upgrades, moving distro
- [docs/FINDINGS.md](docs/FINDINGS.md) — measurements and hardware behaviour
- [docs/EC-LIGHTING.md](docs/EC-LIGHTING.md) — EC lighting endpoint, reverse-engineered
- [docs/UPSTREAM-BUGS.md](docs/UPSTREAM-BUGS.md) — the driver bugs behind the fixes

## Scope

One laptop model. The EC register map, fan curve layout and power limits are specific to
the 83SC. Some of it may apply to other LOQ/Legion models; none of it is verified there.
This model ships in several configs (i5-13450HX or i7-13650HX, RTX 5050 or 5060) — the
numbers here are from the i7 / RTX 5050.

## License

GPL-2.0-or-later.

---

Built by [@blussyya](https://github.com/blussyya) with [Claude Code](https://claude.com/claude-code).
The hardware testing, measurements and decisions are mine; Claude did the driver
reverse-engineering, patches and tooling alongside me.
