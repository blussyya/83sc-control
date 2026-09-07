#!/usr/bin/env python3
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RUNS = os.path.join(os.path.dirname(HERE), "data", "runs")

_tty = sys.stdout.isatty()
def c(code, s): return f"\033[{code}m{s}\033[0m" if _tty else str(s)
BOLD = lambda s: c("1", s); DIM = lambda s: c("2", s)
RED = lambda s: c("31", s); GRN = lambda s: c("32", s); YEL = lambda s: c("33", s)

RATING_COLOR = {"smooth": GRN, "ok": YEL, "bad": RED}

def load():
    out = []
    if not os.path.isdir(RUNS):
        return out
    for d in sorted(os.listdir(RUNS)):
        p = os.path.join(RUNS, d, "meta.json")
        if os.path.exists(p):
            try:
                with open(p) as f:
                    m = json.load(f)
                m["_dir"] = d
                out.append(m)
            except (OSError, json.JSONDecodeError):
                pass
    return out

def main():
    runs = load()
    if not runs:
        print("\n  no runs logged yet. Record one with:\n"
              "    ./harness/run-test.py --preset cool --minutes 12\n")
        return

    print()
    hdr = (f"  {'run':<26} {'PRO':>4} {'strk':>5} {'floor%':>7} "
           f"{'Tmax':>5} {'Tavg':>5} {'Wmax':>5} {'Wavg':>5} {'MHz':>5}  {'felt':<7}")
    print(BOLD(hdr))
    print(DIM("  " + "-" * (len(hdr) - 2)))

    for m in runs:
        s, subj = m.get("summary", {}), m.get("subjective", {})
        rating = subj.get("rating", "?")
        col = RATING_COLOR.get(rating, DIM)
        pro = s.get("prochot_events", 0) or 0
        procol = GRN if pro == 0 else YEL if pro < 20 else RED
        print(f"  {m.get('label', m['_dir'])[:25]:<26} "
              f"{procol(f'{pro:>4}')} "
              f"{s.get('prochot_longest_streak_s', 0) or 0:>5} "
              f"{s.get('floor_pct', 0) or 0:>7} "
              f"{s.get('temp_max_c') or 0:>5.0f} {s.get('temp_avg_c') or 0:>5.0f} "
              f"{s.get('watts_max') or 0:>5.0f} {s.get('watts_avg') or 0:>5.0f} "
              f"{s.get('freq_avg_mhz') or 0:>5.0f}  {col(f'{rating:<7}')}")

    print(DIM("\n  PRO=PROCHOT events  strk=longest consecutive throttling streak (s)"))
    print(DIM("  floor%=samples with a core at/below 500MHz  W=package watts\n"))

    rated = [m for m in runs if m.get("subjective", {}).get("rating") in RATING_COLOR]
    if len(rated) >= 3:
        good = [m["summary"].get("prochot_events", 0) for m in rated
                if m["subjective"]["rating"] == "smooth"]
        bad = [m["summary"].get("prochot_events", 0) for m in rated
               if m["subjective"]["rating"] == "bad"]
        if good and bad:
            gmax, bmin = max(good), min(bad)
            print(BOLD("  does PROCHOT count predict how it felt?"))
            print(f"    worst 'smooth' run: {gmax} events    best 'bad' run: {bmin} events")
            if gmax < bmin:
                print(f"    {GRN('clean separation')} -- optimise for PROCHOT count directly")
            else:
                print(f"    {YEL('overlapping')} -- raw count does not predict feel; "
                      "weight streak length and timing instead")
            print()

    for m in runs:
        subj = m.get("subjective", {})
        if subj.get("when") or subj.get("notes"):
            print(DIM(f"  {m.get('label')}: {subj.get('when','')} {subj.get('notes','')}"))
    print()

if __name__ == "__main__":
    main()
