# Portability: kernel upgrades, and moving to another distro

The stack has two independent halves. Knowing which is which explains what
survives what.

## What actually makes the fix work

1. **The kernel driver** (`legion_laptop`) — exposes the fan curve, power
   limits, and toggles. The 83SC fixes are **upstream** as of commit
   `9af57e3` in johnfanv2/LenovoLegionLinux, so any kernel whose bundled
   `legion-laptop` is new enough already has them.
2. **The boot service** (`83sc-thermal.service`) — at every boot it replays
   `/etc/83sc-control/boot.conf`: powermode, RAPL power limits (the real
   stutter fix), the −100 mV undervolt, GPU limits, and the fan curve. This is
   plain systemd + sysfs and does not depend on the distro.

Everything else (GUI, CLI, idle dimmer) is convenience on top.

## The failure this actually hit (kernel 6.18.48 -> 6.18.50)

Observed in the wild, not theory: the distro package `lenovolegionlinux`
registers its **own** DKMS module `LenovoLegionLinux/1.0.0`, built from an
upstream commit that predates our fixes. Both produce the same
`legion-laptop.ko`. On a kernel upgrade DKMS autoinstalls both and **whichever
installs last wins, silently**. The stock one won:

- `fan2_input` reappeared (the phantom fan), `fan1_max` read 10000 not 5400
- every `pwm1_auto_point*` write failed, temps read back 0 - the original bug
- `83sc-thermal.service` failed; power limits still applied, fan curve did not

**Fix / prevention:** `83sc-driver-guard.sh` detects the stock module (phantom
fan2 present, or `fan1_max != 5400`), drops the stock DKMS registration,
installs ours, and reloads. It runs:

- at boot, via `83sc-driver-guard.service` (ordered `Before=83sc-thermal`), and
- on Arch-likes, immediately post-upgrade via `/etc/pacman.d/hooks/83sc-driver.hook`
  (triggered by kernel, dkms, or lenovolegionlinux transactions).

Manual repair if you ever need it:

```
sudo ./install-dkms.sh && sudo systemctl restart 83sc-thermal
```

Check which module is live at any time:

```
cat /sys/class/hwmon/hwmon*/fan1_max        # 5400 = patched, 10000 = stock
ls /sys/class/hwmon/hwmon*/fan2_input       # present = stock module
```

You can also remove the competing package entirely (`pacman -Rns
lenovolegionlinux`) once you are happy the patched DKMS covers everything.

## Kernel upgrade (same distro)

- **DKMS rebuilds the driver automatically.** `install-dkms.sh` registers the
  patched module with `AUTOINSTALL=yes`, so a new kernel triggers a rebuild.
  It needs `dkms` and the matching kernel headers installed — that's the only
  requirement. It auto-detects clang- vs gcc-built kernels.
- **If your new kernel already ships the upstream fix**, you don't need the
  DKMS override at all. Remove it with `sudo ./install-dkms.sh --remove` and
  rely on the stock module. Check with:
  `grep -c pwm1_auto_point1_temp /sys/class/hwmon/hwmon*/uevent` style probing,
  or just confirm `fan1_max` reads 5400 and point-1 temp is non-zero.
- The boot service is kernel-independent; nothing to do.

## Moving to another distro (e.g. Debian)

Run `sudo ./install-all.sh`. It is distro-agnostic — it checks for what each
step needs and names the Debian packages if something's missing. Manual
equivalent:

1. **Dependencies** (Debian):
   `sudo apt install dkms build-essential linux-headers-$(uname -r) cargo intel-undervolt`
2. **Driver**: `sudo ./install-dkms.sh` — *or* skip it if Debian's kernel
   already carries the upstream fix.
3. **Helper + sudoers**: `sudo ./install.sh`
4. **CLI + curve**: `sudo ./install-cli.sh`
5. **Boot service**: `sudo ./install-thermal.sh`
6. **GUI**: `cd gui && cargo build --release && sudo ./install-gui.sh`
7. **Idle dimmer**: `cd kbd-idle && cargo build --release && ./install-kbd-idle.sh`
8. Copy your `/etc/83sc-control/boot.conf` and `curve.conf` across, or just
   re-apply in the GUI and hit persist.

### Things that are NOT portable as-is — rebuild, don't copy

- **The compiled binaries** (`legion83-gui`, `83sc-kbd-idle`) are linked
  against this machine's glibc. **Do not copy the binaries to Debian** — build
  from source there (`cargo build --release`). The source is in the repo;
  `install-all.sh` builds them for you.
- **The idle dimmer needs a Wayland compositor implementing
  `ext-idle-notify-v1`** (KWin/wlroots do). On X11 or a compositor without it,
  the dimmer won't arm — it fails loudly rather than misbehaving. Everything
  else works regardless of session type.
- **Undervolt** replay needs `intel-undervolt` installed; without it the
  service skips the −100 mV offset and still applies the power limits.

## Quick verification after any of the above

```
83sc-diag | grep -E "PL1 sustained|throttles at"   # power limits live
83sc-fan show                                       # fan curve loaded
systemctl status 83sc-thermal.service               # boot replay OK
systemctl --user status 83sc-kbd-idle               # idle dimmer running
```
