# EC lighting control endpoint (from SECN22WW BIOS + ACPI reversing)

Reverse-engineered from `secn22ww.exe` (Insyde H2O → signed_SE.ROM → UEFI FVs)
and the live ACPI tables. Goal: find a way to control the power LED (and maybe
finer keyboard brightness) **without flashing the EC**.

## The finding

There is a live lighting-control endpoint the Linux driver never touches — the
Legion "GameZone" (GZFD device) lighting-owner protocol. A UEFI DXE driver
("SetLightControlOwner 1 - APP" / "0 - ITE") uses it, so it is wired, not dead.

### EC registers (standard EC space, reachable via our tools)
| field | EC byte | bits | meaning |
|-------|---------|------|---------|
| GZ52  | 0x18    | bit0 | light control owner: 1=APP(host), 0=ITE(firmware) |
| GZ35  | 0x18    | bit1 | (keyboard path gate; SLT2/GLT1 check GZ35==0) |
| CGDB  | 0xf6    | 16b  | control data block (command/value in) |
| GCDB  | 0xf8    | 16b  | control data block (value/out) |

Live baseline: GZ52=0, GZ35=0, CGDB=0, GCDB=0 (ITE owns lighting → this is why
the power LED breathes on its own in S3; the EC animates it).

### The 4 addressable "lights" (DD00 capability table)
| light ID    | value steps | note |
|-------------|-------------|------|
| 0x0107FF00  | 13 (0x14..0xA0) | widest range — candidate for real keyboard dimming beyond the 3 KLEN steps |
| 0x0202FF00  | 4 (0x23,0x28,0x2d,0x32) | |
| 0x0201FF00  | 2 (0x0a,0x0f) | |
| 0x020BFF00  | 2 (0x0a,0x0f) | power LED / logo candidates (small range = indicator) |

### Command shape (GZFD set method, SSDT4)
Request fields DEV1 (device), FEA1 (feature), DAT1 (data), TYP1 (type).
For DEV1=0x02, FEA1=0x01:  `CGDB = DAT1`  then Notify(NPCF,0xC0).
Ownership is set separately: WMI method Arg1=0x34 writes GZ52.

## Why this matters
All of GZ52/CGDB/GCDB are in the EC space our helper already reads/writes
(`dump-ec`, `ec-write`). So IF driving this protocol controls the power LED,
we get it with **no EC firmware flash** — pure EC-register writes, exactly the
"leftover endpoint" theory.

## RESULT (tested 2026-09-12) - parked

The endpoint is real but **not wired to the power LED on this model**.

What was wrong the first time: `GZ52`/`CGDB`/`GCDB` live in the **`ECMM`
region, `SystemMemory` at `0xFE0B0400`** - not in the `EmbeddedControl` port
space that `ec_sys` and our `ec-write` helper target. The firmware declares
`ECAM` (EmbeddedControl) with a completely **empty field**; nothing lives
there. Every early probe wrote the wrong address space, which is why writes
appeared to "revert" and nothing ever lit up. Those results were void, not
negative.

Re-tested properly via `/dev/mem` at `0xFE0B0400` (window confirmed: byte
`0x03` tracks the keyboard `KLEN` exactly):

- `GZ52 = 1` (take APP ownership) **sticks** - it is a real, writable register.
- Under ownership, sweeping `CGDB` (0x01..0xff) and `GCDB`: **no visible
  change** to the power LED or the rear I/O light.
- Only `KLEN` moves the keyboard, i.e. the path we already had.

Conclusion: these are the Legion RGB-variant lighting registers, present in
shared firmware but not connected to this chassis's indicators - the same
pattern as the phantom `fan2`, the empty `SLT2` cases 4/5, and the absent
Spectrum WMI GUID. The power LED stays EC-internal with **no host-reachable
control path**; changing it would require patched EC firmware.

Keyboard brightness remains 3 usable levels (`KLEN` is a 2-bit field; value 3
is electrically identical to 2, confirmed by eye in both address spaces).
Bypassing UPower to write the EC directly reaches the same four codes - the
limit is the field width, not the API.

Practical answer for the power LED: a physical dimming sticker.

## If ever resumed - next steps
1. Which light ID is the power LED? (0x020B / 0x0201 are the small-range
   indicator candidates.) Map by experiment.
2. Does taking APP ownership (GZ52=1) alone change the power LED? (Minimal,
   reversible probe.)
3. Decode the exact DEV1/FEA1/DAT1 packing to issue a brightness for a given
   light, then replay via EC writes with the physical LED watched.
4. Whether 0x0107FF00's 13 steps give finer *keyboard* brightness than KLEN.

## Artifacts
EC firmware image and BIOS extraction live in the session scratchpad (not in
git — vendor firmware). Re-extract from `secn22ww.exe` with:
`7z x` → `signed_SE.ROM` → carve/LZMA-decompress → UEFI FV at 0x1000.
