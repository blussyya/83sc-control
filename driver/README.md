# legion_laptop driver source

`legion-laptop.c` and `Makefile` are copied unmodified from
[johnfanv2/LenovoLegionLinux](https://github.com/johnfanv2/LenovoLegionLinux)
tag `v0.0.26` (commit `e3b2116`), `kernel_module/`. That is the first release
carrying the 83SC fixes (single fan, per-model fan_max, fan curve readback).
GPL-2.0-or-later, same as this repo.

Local changes live in `../patches/` and are applied at install/package time by
`packaging/stage-driver.sh`, so this copy stays diffable against upstream.

To move to a newer upstream: replace these two files, then check that
`packaging/stage-driver.sh /tmp/x` still applies every patch cleanly.
