# Sourceable: starts and stops the jailed portal chain on a private bus:
#   pipewire, wireplumber, transom, xdg-permission-store, xdg-desktop-portal (which finds transom
#   through dist/quire.portal).
# Everything runs in the jail's scratch; nothing touches the real session. Callers have already
# run require_jail. Owner: P2-Z-tr (skeleton), the P2-T lanes fill the details they need.
#
# State lives in $TRANSOM_STATE (default $XDG_STATE_HOME/transom-dev/<run>): an `env` file with
# the exports and one pid per line. Processes are stopped by pid, never by name.

# Where binaries come from. TRANSOM_BIN: the built transom; the system ones are looked up on PATH.
transom_tool() {
  local name=$1
  case "$name" in
    transom)
      if [ -n "${TRANSOM_BIN:-}" ] && [ -x "$TRANSOM_BIN" ]; then echo "$TRANSOM_BIN"; return 0; fi
      echo "chain.sh: set TRANSOM_BIN to the built transom binary" >&2; return 3 ;;
    xdg-desktop-portal)
      for p in /usr/libexec/xdg-desktop-portal /usr/lib/xdg-desktop-portal /usr/libexec/xdg-desktop-portal; do
        if [ -x "$p" ]; then echo "$p"; return 0; fi
      done
      echo "chain.sh: xdg-desktop-portal is not installed" >&2; return 3 ;;
    xdg-permission-store)
      # Fedora and Debian/Ubuntu: /usr/libexec; Arch: /usr/lib.
      for p in /usr/libexec/xdg-permission-store /usr/lib/xdg-permission-store; do
        if [ -x "$p" ]; then echo "$p"; return 0; fi
      done
      echo "chain.sh: xdg-permission-store is not installed" >&2; return 3 ;;
    *)
      if command -v "$name" >/dev/null 2>&1; then command -v "$name"; return 0; fi
      echo "chain.sh: $name is not installed" >&2; return 3 ;;
  esac
}

transom_state_dir() {
  echo "${TRANSOM_STATE:?chain.sh: TRANSOM_STATE is not set}"
}

# start_chain [--env FILE]: with --env, join the compositor session that env file describes (its
# WAYLAND_DISPLAY and bus); without, start a private bus and no compositor.
start_chain() {
  require_jail
  local root state envfile=""
  root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
  while [ $# -gt 0 ]; do
    case "$1" in
      --env) envfile=${2:?--env needs a file}; shift 2 ;;
      *) echo "start_chain: unknown argument $1" >&2; return 3 ;;
    esac
  done
  : "${XDG_RUNTIME_DIR:?start_chain: XDG_RUNTIME_DIR is not set}"
  if [ -z "${TRANSOM_STATE:-}" ]; then
    TRANSOM_STATE="${XDG_STATE_HOME:?start_chain: XDG_STATE_HOME is not set}/transom-dev/$$"
    export TRANSOM_STATE
  fi
  state=$TRANSOM_STATE
  mkdir -p "$state/portals" "$XDG_CONFIG_HOME/xdg-desktop-portal"

  local pw wp tr ps xdp
  pw=$(transom_tool pipewire) || return 3
  wp=$(transom_tool wireplumber) || return 3
  tr=$(transom_tool transom) || return 3
  ps=$(transom_tool xdg-permission-store) || return 3
  xdp=$(transom_tool xdg-desktop-portal) || return 3

  if [ -n "$envfile" ]; then
    # The env file holds plain assignments (the bus address, the compositor's display); `set -a`
    # exports them, or the children started below would not see the session they join.
    set -a
    # shellcheck source=/dev/null
    source "$envfile"
    set +a
  else
    private_bus_start || return 1
    echo "TRANSOM_BUS_PID=$PRIVATE_BUS_PID" >> "$state/env"
  fi

  cp "$root/dist/quire.portal" "$state/portals/quire.portal"
  cp "$root/dist/quire-portals.conf" "$XDG_CONFIG_HOME/xdg-desktop-portal/quire-portals.conf"
  export XDG_CURRENT_DESKTOP=quire XDG_DESKTOP_PORTAL_DIR="$state/portals"

  "$pw" >"$state/pipewire.log" 2>&1 & echo "TRANSOM_PIPEWIRE_PID=$!" >> "$state/env"
  "$wp" >"$state/wireplumber.log" 2>&1 & echo "TRANSOM_WIREPLUMBER_PID=$!" >> "$state/env"
  "$tr" >"$state/transom.log" 2>&1 & echo "TRANSOM_PID=$!" >> "$state/env"
  # The frontend records the person's answers (Access, screen sharing) in the permission store and
  # gives up on the request when it cannot reach one; the private bus has no activation for it.
  "$ps" --replace >"$state/permission-store.log" 2>&1 & echo "TRANSOM_PERMISSION_STORE_PID=$!" >> "$state/env"
  "$xdp" >"$state/portal.log" 2>&1 & echo "TRANSOM_PORTAL_PID=$!" >> "$state/env"
  # `export` on each line: callers source this file in their own shell and start more clients
  # (a fake shell, a cast) that must find the same bus.
  {
    printf 'export DBUS_SESSION_BUS_ADDRESS=%q\n' "$DBUS_SESSION_BUS_ADDRESS"
    echo "export XDG_CURRENT_DESKTOP=quire"
    printf 'export XDG_DESKTOP_PORTAL_DIR=%q\n' "$state/portals"
    printf 'export TRANSOM_STATE=%q\n' "$state"
  } >> "$state/env"

  # Wait (up to 10 s) for the portal frontend to own its name.
  local tenths=0
  until gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
      --method org.freedesktop.DBus.NameHasOwner org.freedesktop.portal.Desktop 2>/dev/null | grep -q true; do
    tenths=$((tenths + 1))
    if [ "$tenths" -gt 100 ]; then echo "start_chain: the portal did not come up; see $state/*.log" >&2; return 1; fi
    sleep 0.1
  done
}

# stop_chain: stop what start_chain started, each by the pid in the env file, the bus last.
stop_chain() {
  local state envfile var pid
  state=$(transom_state_dir) || return 0
  envfile=$state/env
  [ -f "$envfile" ] || return 0
  for var in TRANSOM_PORTAL_PID TRANSOM_PERMISSION_STORE_PID TRANSOM_PID TRANSOM_WIREPLUMBER_PID TRANSOM_PIPEWIRE_PID TRANSOM_BUS_PID; do
    pid=$(sed -n "s/^$var=//p" "$envfile" | tail -n 1)
    if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; fi
  done
}
