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

grab the package for your distro from
[Releases](https://github.com/blussyya/83sc-control/releases/latest), it has everything:
the GUI, CLI, boot service, and the patched `legion_laptop` driver (built by DKMS
for your kernel, rebuilt on every kernel update). services are enabled on install

you need your kernel's headers installed so DKMS can build the driver

**Arch / CachyOS**

```bash
sudo pacman -S --needed dkms linux-headers   # or the headers for your kernel, e.g. linux-cachyos-lts-headers
sudo pacman -U 83sc-control-*.pkg.tar.zst
```

**Debian / Ubuntu**

```bash
sudo apt install ./83sc-control_*_amd64.deb
```

**Fedora**

```bash
sudo dnf install ./83sc-control-*.x86_64.rpm
```

with Secure Boot on, the driver has to be signed: Debian/Ubuntu's dkms prompts you to
enroll its key (MOK) on first install, reboot once and accept it. on Fedora/Arch set up
dkms signing or the module wont load

**from source** (any distro)

```bash
./setup.sh
```

run `setup.sh` as your normal user, it elevates for the privileged parts and drops back
to your session for the user service. `./setup.sh --remove` undoes it all.
to build the packages yourself: `./packaging/build-all.sh`

### releasing

bump `VERSION` and push to master, CI builds all three packages and publishes them as
release `v<VERSION>`. every other push still builds them (under the run's artifacts)

### After installing

set what you want in **83SC Control** (app menu). with "restore at boot" ticked, whatever
you apply is replayed at boot

## requirements

- the packages ship the driver, nothing else to install for fan control. from source,
  `install-dkms.sh` does the same (`driver/` is upstream LenovoLegionLinux v0.0.26 plus
  `patches/`)
- `intel-undervolt` (optional) to apply a voltage offset at boot if you wanna undervolt
- power limits work on any kernel, they use mainline `intel_rapl`, not a lenovo driver
- the GUI works on any Wayland or X11 desktop. the idle dimmer needs a Wayland
  compositor with `ext-idle-notify-v1` (KDE, Hyprland, Sway, niri, COSMIC), and profile
  binding to power plans needs `power-profiles-daemon` (or `tuned-ppd`)

## whats in the package

| | |
|---|---|
| `83SC Control` | GUI: fan curve, power limits, undervolt, profiles bound to power plans |
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
