#!/usr/bin/env bash
# Acceptance script 'settings': ashpd-style reads of org.freedesktop.appearance through the portal.
# Prints PASS, SKIP or FAIL per step. Calls require_jail first.
#
# Owner: P2-T1. Skeleton: the steps below are written as comments and exit 2 (not runnable yet)
# until the lane fills them.
#   exit 0 all steps passed, 1 a step failed, 2 not runnable (outside the jail, or not filled yet),
#   3 a tool is missing.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"

# 1. dev/up.sh (no compositor needed).
# 2. Read color-scheme, accent-color and contrast with org.freedesktop.portal.Settings.Read.
# 3. Change the desktop settings document; SettingChanged arrives with the new value.
# 4. dev/down.sh.

echo "SKIP settings: not filled yet (P2-T1)"
exit 2
