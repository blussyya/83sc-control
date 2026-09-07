#!/usr/bin/env bash
set -euo pipefail

PL1_WATTS=45
PL2_WATTS=90
TAU_SECONDS=8
POWERMODE=255
FAN_FULLSPEED=1

LEGION=/sys/bus/platform/devices/PNP0C09:00
RAPL=/sys/class/powercap/intel-rapl:0

put() {
    local path=$1 value=$2
    [[ -w $path ]] || { echo "83sc-thermal: $path not writable, skipping" >&2; return 0; }
    echo "$value" > "$path" 2>/dev/null || echo "83sc-thermal: write failed: $path" >&2
}

put "$LEGION/powermode"   "$POWERMODE"
put "$LEGION/fan_fullspeed" "$FAN_FULLSPEED"

put "$RAPL/constraint_0_power_limit_uw" "$((PL1_WATTS * 1000000))"
put "$RAPL/constraint_1_power_limit_uw" "$((PL2_WATTS * 1000000))"
put "$RAPL/constraint_0_time_window_us" "$((TAU_SECONDS * 1000000))"

pl1=$(cat "$RAPL/constraint_0_power_limit_uw" 2>/dev/null || echo 0)
pl2=$(cat "$RAPL/constraint_1_power_limit_uw" 2>/dev/null || echo 0)
tau=$(cat "$RAPL/constraint_0_time_window_us" 2>/dev/null || echo 0)
echo "83sc-thermal: PL1=$((pl1 / 1000000))W PL2=$((pl2 / 1000000))W tau=$((tau / 1000000))s" \
     "powermode=$(cat "$LEGION/powermode" 2>/dev/null)" \
     "fan_fullspeed=$(cat "$LEGION/fan_fullspeed" 2>/dev/null)"
