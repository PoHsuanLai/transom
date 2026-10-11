#!/usr/bin/env bash
# Acceptance script 'screenshot': a Screenshot portal call through xdg-desktop-portal returns a PNG of the headless output.
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

# 1. dev/up.sh --env <compositor env file>.
# 2. Call org.freedesktop.portal.Screenshot.Screenshot with gdbus and wait for the Response signal.
# 3. The reply carries a file: uri; the file is a PNG whose size equals the output size at its scale.
# 4. dev/down.sh.

echo "SKIP screenshot: not filled yet (P2-T1)"
exit 2
