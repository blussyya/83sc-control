#!/usr/bin/env bash
#
# Undervolt stability + benefit sweep.
#
#   sudo ./undervolt-sweep.sh [max_mv] [seconds_per_step]
#   sudo ./undervolt-sweep.sh 100 45        # all-core
#   sudo ./undervolt-sweep.sh 100 45 4      # 4 threads, closer to a game
#
# Steps -25, -50, -75 ... up to max_mv. At each step it applies the offset, runs
# an all-core load at the CURRENT power limit, and records average clock and
# temperature. Undervolting is only a win if clocks rise at the same wattage --
# with CEP enabled they may not, so this measures rather than assumes.
#
# SAFETY
#   - The offset is never made persistent. /etc/intel-undervolt.conf is restored
#     on exit, and the intel-undervolt systemd service is never enabled, so a
#     hang is recovered by simply rebooting.
#   - Machine check counters are read before and after each step; a new MCE aborts
#     the sweep and reverts.
#   - An unstable undervolt usually hard-locks rather than erroring. If this
#     script never prints its summary, reboot and use a smaller max_mv.
set -uo pipefail

MAXMV=${1:-100}
DUR=${2:-45}
THREADS=${3:-$(nproc)}
COOL=${4:-40}
CONF=/etc/intel-undervolt.conf
BACKUP=$(mktemp)
RESULTS=()

[[ $EUID -eq 0 ]] || { echo "run as root: sudo ./undervolt-sweep.sh" >&2; exit 1; }
command -v intel-undervolt >/dev/null || { echo "intel-undervolt not installed" >&2; exit 1; }
command -v stress-ng >/dev/null || { echo "stress-ng not installed" >&2; exit 1; }

cp "$CONF" "$BACKUP"
cleanup() {
    sed -i -E "s/^enable .*/enable no/; s/^undervolt 0 'CPU' .*/undervolt 0 'CPU' 0/; s/^undervolt 1 'GPU' .*/undervolt 1 'GPU' 0/; s/^undervolt 2 'CPU Cache' .*/undervolt 2 'CPU Cache' 0/" "$CONF"
    intel-undervolt apply >/dev/null 2>&1 || true
    cp "$BACKUP" "$CONF"; rm -f "$BACKUP"
    echo
    echo "  reverted to 0 mV, $CONF restored, nothing persistent"
}
trap cleanup EXIT INT TERM

mce_log()   { dmesg 2>/dev/null | grep -ci "machine check\|mce:" || :; }

coretemp=""
for h in /sys/class/hwmon/hwmon*; do
    [[ "$(cat "$h/name" 2>/dev/null)" == coretemp ]] && coretemp="$h" && break
done

measure() {   # -> "avgMHz avgC maxC avgW"
    local n=0 sumf=0 sumt=0 maxt=0 f t
    local e0 e1 t0 t1 w
    local EJ=/sys/class/powercap/intel-rapl:0/energy_uj
    local end=$((SECONDS + DUR - 5))
    stress-ng --cpu "$THREADS" --timeout "${DUR}s" >/dev/null 2>&1 &
    local pid=$!
    sleep 5   # let the load settle and PL1 engage
    e0=$(cat $EJ 2>/dev/null || echo 0); t0=$SECONDS
    while (( SECONDS < end )); do
        f=$(awk '{s+=$1;n++}END{printf "%.0f", s/n/1000}' /sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq)
        t=$(( $(cat "$coretemp/temp1_input" 2>/dev/null || echo 0) / 1000 ))
        sumf=$((sumf + f)); sumt=$((sumt + t)); n=$((n + 1))
        (( t > maxt )) && maxt=$t
        sleep 2
    done
    e1=$(cat $EJ 2>/dev/null || echo 0); t1=$SECONDS
    wait $pid 2>/dev/null
    (( n == 0 )) && { echo "0 0 0 0"; return; }
    w=0; (( t1 > t0 && e1 > e0 )) && w=$(( (e1 - e0) / 1000000 / (t1 - t0) ))
    echo "$((sumf / n)) $((sumt / n)) $maxt $w"
}

PL1=$(( $(cat /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw) / 1000000 ))
echo
echo "  sweep to -${MAXMV} mV, ${DUR}s per step, PL1 = ${PL1} W, ${THREADS} threads"
echo "  ${COOL}s cooldown between steps; core and cache offset together"
echo "  a win means higher MHz at the same watts"
echo

BASE_MCE=$(mce_log)

printf "  %-10s %-10s %-10s %-8s %-8s\n" offset avgMHz avgC maxC avgW
for (( mv=0; mv<=MAXMV; mv+=25 )); do
    sed -i -E "s/^enable .*/enable yes/; s/^undervolt 0 'CPU' .*/undervolt 0 'CPU' -${mv}/; s/^undervolt 2 'CPU Cache' .*/undervolt 2 'CPU Cache' -${mv}/" "$CONF"
    if ! intel-undervolt apply >/dev/null 2>&1; then
        echo "  apply failed at -${mv} mV, stopping"; break
    fi
    got=$(intel-undervolt read | awk -F': *' '/^CPU \(0\)/{print $2}')
    # let the chassis settle so every step starts from a comparable temperature
    printf "  cooling..\r"; sleep "$COOL"
    read -r mhz avgc maxc watt <<< "$(measure)"
    printf "  %-10s %-10s %-10s %-8s %-8s  (reads %s)\n" "-${mv} mV" "$mhz" "$avgc" "$maxc" "${watt}W" "$got"
    RESULTS+=("$mv $mhz $avgc $maxc $watt")

    if [[ $(mce_log) -gt $BASE_MCE ]]; then
        echo "  !! machine check logged at -${mv} mV - unstable, stopping"; break
    fi
done

echo
echo "  summary"
best_mv=0; best_mhz=0
for r in "${RESULTS[@]}"; do
    read -r mv mhz _ _ _ <<< "$r"
    (( mhz > best_mhz )) && { best_mhz=$mhz; best_mv=$mv; }
done
read -r _ base_mhz base_c _ base_w <<< "${RESULTS[0]:-0 0 0 0 0}"
read -r _ _ last_c _ last_w <<< "${RESULTS[-1]:-0 0 0 0 0}"
gain=$(( best_mhz - base_mhz ))
pct=0
(( base_mhz > 0 )) && pct=$(( gain * 100 / base_mhz ))
echo "  baseline ${base_mhz} MHz, best -${best_mv} mV at ${best_mhz} MHz (+${gain} MHz, ${pct}%)"
# run-to-run spread on this rig is ~2%, so anything under that is not a result
if (( pct >= 3 )); then
    echo "  REAL GAIN - undervolting helps here, worth making persistent"
elif (( pct >= 1 )); then
    echo "  WITHIN NOISE - ${pct}% is below run-to-run variance, not a result"
else
    echo "  no clock gain"
fi
echo
echo "  temperature: ${base_c}C -> ${last_c}C   package power: ${base_w}W -> ${last_w}W"
# If watts fall with the offset the chip was not actually at its power ceiling,
# so the run measured a thermal limit rather than a power-limited clock.
if (( base_w > 0 && base_w - last_w >= 3 )); then
    echo "  power DROPPED - the run was thermally limited, not power limited"
elif (( base_c - last_c >= 5 )); then
    echo "  same watts, cooler - undervolting buys thermal headroom at equal power"
fi
