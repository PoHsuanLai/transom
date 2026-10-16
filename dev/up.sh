#!/usr/bin/env bash
# Starts the jailed portal chain: a private bus, pipewire, wireplumber, transom and
# xdg-desktop-portal, all inside the jail's scratch. Prints the run's env file on stdout; source
# it to talk to the chain. Calls require_jail first.
#
#   dev/up.sh [--env COMPOSITOR_ENV_FILE]
#
# With --env, joins the headless compositor session that file describes (the capture checks need
# one); without, there is no compositor and only the Settings and Inhibit checks can run.
# Needs TRANSOM_BIN (the built transom binary). TRANSOM_DEV_CALLERS (a rows file naming the
# stand-ins for the portal frontend and the shell; transom built with --features debug) is passed on
# to transom; see dev/lib/chain.sh.
#
# Owner: P2-Z-tr (skeleton). Exit codes: 0 up, 1 the chain did not come up, 2 outside the jail,
# 3 a tool is missing.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"
start_chain "$@"
echo "$(transom_state_dir)/env"
