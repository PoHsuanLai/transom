#!/usr/bin/env bash
# Acceptance script 'remote-desktop': RemoteDesktop events reach the compositor tagged as remote.
# Prints PASS, SKIP or FAIL per step. Calls require_jail first.
#
# Owner: P2-T3. Still a skeleton (FINDINGS 'p2-t3: RemoteDesktop'): it needs a stub org.quire.Shell1 on
# the bus, casement's debug peer-unit override so the jailed transom reads as transom.service, and a
# client that receives a file descriptor over D-Bus and speaks EIS.
#   exit 0 all steps passed, 1 a step failed, 2 not runnable (outside the jail, or not filled yet),
#   3 a tool is missing.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"

# 1. dev/up.sh --env <compositor env file> (a headless casement with its debug peer-unit table
#    mapping transom's pid to transom.service), and a stub Shell1 that answers a pick.
# 2. org.freedesktop.portal.RemoteDesktop through xdg-desktop-portal: CreateSession, SelectDevices
#    (types=3), Start (wait for Response 0, devices=3), ConnectToEIS (an fd comes back).
# 3. On that fd, an EIS handshake, bind keyboard and pointer, send a key and a pointer motion;
#    casement's debug log (or Debug1) reports both with the remote origin and the session's number.
# 4. A second ConnectToEIS before Start is refused; a process not named transom.service gets
#    AccessDenied from org.quire.Compositor1.ConnectEis.
# 5. dev/down.sh.

echo "SKIP remote-desktop: needs a stub shell, the peer-unit override and an EIS client (P2-T3)"
exit 2
