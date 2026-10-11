#!/usr/bin/env bash
# Stops what dev/up.sh started, by the pids in the run's env file (never by pattern).
#
#   dev/down.sh ENV_FILE
#
# Owner: P2-Z-tr (skeleton). Exit codes: 0 stopped, 2 outside the jail.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"
TRANSOM_STATE=$(dirname "${1:?usage: dev/down.sh ENV_FILE}")
export TRANSOM_STATE
stop_chain
echo "the portal chain is down"
