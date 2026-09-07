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

if [[ -e $LEGION/powermode ]]; then
    put "$LEGION/powermode" "$POWERMODE" "powermode" || rc=1
    put "$LEGION/fan_fullspeed" "$FAN_FULLSPEED" "fan_fullspeed" || rc=1
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
