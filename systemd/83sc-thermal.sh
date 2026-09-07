#!/usr/bin/env bash
set -uo pipefail

LEGION=/sys/bus/platform/devices/PNP0C09:00
RAPL=/sys/class/powercap/intel-rapl:0
WAIT_SECONDS=15

PROFILE=${1:-game}

case "$PROFILE" in
    quiet)       POWERMODE=1;   PL1_WATTS=25; PL2_WATTS=45; TAU_SECONDS=8;  FAN_FULLSPEED=0 ;;
    balanced)    POWERMODE=2;   PL1_WATTS=35; PL2_WATTS=65; TAU_SECONDS=8;  FAN_FULLSPEED=0 ;;
    performance) POWERMODE=3;   PL1_WATTS=45; PL2_WATTS=90; TAU_SECONDS=8;  FAN_FULLSPEED=0 ;;
    game)        POWERMODE=255; PL1_WATTS=45; PL2_WATTS=90; TAU_SECONDS=8;  FAN_FULLSPEED=1 ;;
    stock)       POWERMODE=3;   PL1_WATTS=55; PL2_WATTS=130; TAU_SECONDS=56; FAN_FULLSPEED=0 ;;
    *)
        echo "usage: ${0##*/} [quiet|balanced|performance|game|stock]" >&2
        exit 2 ;;
esac

log()  { echo "83sc-thermal: $*"; }
fail() { echo "83sc-thermal: $*" >&2; }

rc=0

if [[ ! -e $LEGION/powermode ]]; then
    log "legion_laptop not bound, loading it"
    if ! out=$(modprobe legion_laptop 2>&1); then
        log "modprobe legion_laptop failed: ${out:-no output}, retrying with force=1"
        if ! out=$(modprobe legion_laptop force=1 2>&1); then
            fail "modprobe legion_laptop force=1 failed: ${out:-no output}"
        fi
    fi
fi

waited=0
while [[ ! -w $LEGION/powermode && $waited -lt $WAIT_SECONDS ]]; do
    sleep 1
    waited=$((waited + 1))
done

put() {
    local path=$1 value=$2 what=$3
    if [[ ! -e $path ]]; then
        fail "$what: $path does not exist"
        return 1
    fi
    if ! { echo "$value" > "$path"; } 2>/dev/null; then
        fail "$what: write of '$value' to $path failed"
        return 1
    fi
    return 0
}

CURVE_CONF=/etc/83sc-control/curve.conf

find_hwmon() {
    local h
    for h in /sys/class/hwmon/hwmon*; do
        [[ "$(cat "$h/name" 2>/dev/null)" == legion_hwmon ]] && { echo "$h"; return 0; }
    done
    return 1
}

# The EC reloads its fan table on every power mode change, so the saved curve
# has to be written after the mode is set, not before.
apply_curve() {
    local h temp rpm mx pwm i
    h=$(find_hwmon) || { fail "legion_hwmon not found, curve not applied"; return 1; }
    mx=$(cat "$h/fan1_max" 2>/dev/null || echo 5400)
    [[ $mx -gt 0 ]] || mx=5400

    mapfile -t lines < <(grep -vE '^\s*(#|$)' "$CURVE_CONF")
    [[ ${#lines[@]} -eq 10 ]] || { fail "curve.conf needs exactly 10 points, got ${#lines[@]}"; return 1; }

    # Curve writes are silently ignored outside custom powermode (255), so a
    # profile that selects any other mode cannot also carry a custom curve.
    if [[ "$(cat "$LEGION/powermode" 2>/dev/null)" != "255" ]]; then
        log "curve.conf present, forcing powermode 255 (writes are ignored otherwise)"
        put "$LEGION/powermode" 255 "powermode" || rc=1
        sleep 1
    fi

    # fan_fullspeed pins the fan to maximum and overrides the curve entirely.
    put "$LEGION/fan_fullspeed" 0 "fan_fullspeed" || true
    [[ -e $h/minifancurve ]] && put "$h/minifancurve" 0 "minifancurve" || true

    # Highest point first: trip temperatures must increase monotonically, and
    # writing low-to-high can transiently invert a pair and get rejected.
    for (( i=9; i>=0; i-- )); do
        read -r temp rpm <<< "${lines[$i]}"
        pwm=$(( (rpm * 255 + mx / 2) / mx ))
        (( pwm > 255 )) && pwm=255
        (( pwm < 0 )) && pwm=0
        put "$h/pwm1_auto_point$((i+1))_temp" "$temp" "curve[$((i+1))].temp" || rc=1
        put "$h/pwm1_auto_point$((i+1))_temp_hyst" "$(( temp > 5 ? temp - 5 : 0 ))" "curve[$((i+1))].hyst" || true
        put "$h/pwm1_auto_point$((i+1))_pwm" "$pwm" "curve[$((i+1))].pwm" || rc=1
    done
    log "applied saved fan curve from $CURVE_CONF"
}

if [[ -e $LEGION/powermode ]]; then
    put "$LEGION/powermode" "$POWERMODE" "powermode" || rc=1
    if [[ -r $CURVE_CONF ]]; then
        sleep 1
        apply_curve || rc=1
    else
        put "$LEGION/fan_fullspeed" "$FAN_FULLSPEED" "fan_fullspeed" || rc=1
    fi
else
    fail "legion_laptop did not appear within ${WAIT_SECONDS}s; powermode and fan control NOT applied"
    rc=1
fi

if [[ -d $RAPL ]]; then
    put "$RAPL/constraint_0_power_limit_uw" "$((PL1_WATTS * 1000000))" "PL1" || rc=1
    put "$RAPL/constraint_1_power_limit_uw" "$((PL2_WATTS * 1000000))" "PL2" || rc=1
    put "$RAPL/constraint_0_time_window_us" "$((TAU_SECONDS * 1000000))" "tau" || rc=1
else
    fail "intel-rapl absent; power limits NOT applied - this is the actual stutter fix"
    rc=1
fi

pl1=$(cat "$RAPL/constraint_0_power_limit_uw" 2>/dev/null || echo 0)
pl2=$(cat "$RAPL/constraint_1_power_limit_uw" 2>/dev/null || echo 0)
tau=$(cat "$RAPL/constraint_0_time_window_us" 2>/dev/null || echo 0)
pm=$(cat "$LEGION/powermode" 2>/dev/null || echo "n/a")
ff=$(cat "$LEGION/fan_fullspeed" 2>/dev/null || echo "n/a")

log "profile=$PROFILE PL1=$((pl1 / 1000000))W PL2=$((pl2 / 1000000))W tau=$((tau / 1000000))s powermode=$pm fan_fullspeed=$ff"

if [[ $rc -ne 0 ]]; then
    fail "one or more settings did not apply"
fi
exit $rc
