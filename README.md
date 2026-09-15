# 83sc-control

thermals, power and fan control for the **Lenovo LOQ Essential 15IRX11** (`83SC`,
i7-13650HX/RTX 5050).

stock firmware runs PL1 at 55W sustained on a single-fan, the CPU climbs to
97C, then duty-cycles via T-states, userspace samples that as ~400 MHz, below
`scaling_min_freq`, so it looks like a governor bug but it isnt, in CS2 it showed up
as multi-second stalls.

dropping PL1 to 45W with a 8s window holds 70–72C and no throttling under a
14-thread load, and no PROCHOT events across 240 seconds of combined CPU+GPU load

## Install

**Arch**

```bash
cd packaging && makepkg -si
```

**Debian/Ubuntu**

```bash
./packaging/build-deb.sh && sudo apt install ./dist/83sc-control_1.0.0_amd64.deb
```

**Fedora**

```bash
rpmbuild -ta 83sc-control-1.0.0.tar.gz   # spec: packaging/83sc-control.spec
```

**from source**

```bash
./setup.sh
```

run `setup.sh` as your normal user, it elevates for the privileged parts and drops back
to your session for the user service
re-run it after a kernel upgrade as it wont stick
build every package your machine has tooling for: `./packaging/build-all.sh`
`./setup.sh --remove` undoes it all


### After installing

```bash
sudo systemctl enable --now 83sc-driver-guard 83sc-thermal
systemctl --user enable --now 83sc-kbd-idle
```

set what you want in **83SC Control** (app menu), then `sudo 83sc boot-save` to have it
replay at boot.

## requirements

- fan control needs `legion_laptop` with the 83SC fixes — **LenovoLegionLinux ≥ v0.0.26**.
  older versions return zeroed fan-curve temps and reject writes
- `intel-undervolt` (optional) to apply a voltage offset at boot if you wanna undervolt
- power limits work on any kernel, they use mainline `intel_rapl`, not a lenovo driver

## whats in the package

| | |
|---|---|
| `83SC Control` | GUI: fan curve, power limits, undervolt, profiles bound to KDE power plans |
| `83sc-fan` | show or set the fan curve |
| `83sc-diag` | thermal and power-limit diagnostics |
| `83sc-snap` | one-shot state dump |
| `83sc` | profile and boot-state management |
| `83sc-thermal.service` | replays power limits, undervolt and fan curve at boot |
| `83sc-driver-guard.service` | keeps the fixed driver loaded across kernel upgrades |
| `83sc-kbd-idle` | keyboard backlight off after N seconds idle |

the GUI and CLI never run as root

### driver guard

distro packages may ship a `legion_laptop` built from a pre-fix commit. both produce the
same `legion-laptop.ko`, so on a kernel upgrade whichever DKMS installs last sticks
when the stock one installs last  the phantom `fan2` returns and fan-curve writes fail.
the guard detects that by its symptoms and repairs it, at boot and (on Arch) immediately
post-upgrade, it stands down once the packaged module carries the fixes

## upstream

five kernel fixes from this work got merged in
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

one laptop model has been tested here, the fan curve layout and power limits in here are specific to
the 83SC since thats what i worked with. some of it may apply to other LOQ models 
this model ships in several configs (i5-13450HX or i7-13650HX, RTX 5050 or 5060) the
numbers here are from the i7/RTX 5050

## License

GPL-2.0-or-later.
try under your own caution i am not responsible for any breakage
---

claude code has been used in this project if ur concerned about ai code
the hardware testing, measurements and decisions are mine, claude did the driver
reverse-engineering, patches and tooling alongside me
