# Sourceable helpers for the screencast and chooser checks. Callers have already run
# require_jail and sourced chain.sh. The compositor session comes from $TRANSOM_COMPOSITOR_ENV
# (an env file as dev/up.sh --env takes); a window check also needs $TRANSOM_TEST_WINDOW, the
# identifier of a window open in that session. Unset: the check exits 2 (not runnable).

step() { echo "$1 $2"; }

need_compositor() {
  if [ -z "${TRANSOM_COMPOSITOR_ENV:-}" ] || [ ! -f "$TRANSOM_COMPOSITOR_ENV" ]; then
    echo "SKIP $1: set TRANSOM_COMPOSITOR_ENV to a headless compositor's env file"
    exit 2
  fi
}

# start_fake_shell ANSWER...: own org.quire.Shell in the background; sets SHELL_PID and SHELL_LOG.
start_fake_shell() {
  SHELL_LOG=$(transom_state_dir)/fake-shell.log
  : > "$SHELL_LOG"
  python3 "$root/dev/lib/portal_client.py" fake-shell "$SHELL_LOG" "$@" &
  SHELL_PID=$!
  local tenths=0
  until gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
      --method org.freedesktop.DBus.NameHasOwner org.quire.Shell 2>/dev/null | grep -q true; do
    tenths=$((tenths + 1))
    [ "$tenths" -gt 50 ] && { echo "FAIL: the fake shell did not come up"; return 1; }
    sleep 0.1
  done
}

# run_cast TYPES PERSIST [TOKEN]: run a cast in the background; sets CAST_PID and CAST_OUT (the
# one result line lands in the file). The cast holds its session open on a fifo until stop_cast.
run_cast() {
  local dir; dir=$(transom_state_dir)
  CAST_OUT=$dir/cast-$RANDOM.out
  CAST_IN=$dir/cast-$RANDOM.in
  mkfifo "$CAST_IN"
  python3 "$root/dev/lib/portal_client.py" cast "$@" < "$CAST_IN" > "$CAST_OUT" &
  CAST_PID=$!
  exec {CAST_FD}> "$CAST_IN"
  local tenths=0
  until [ -s "$CAST_OUT" ]; do
    tenths=$((tenths + 1))
    [ "$tenths" -gt 300 ] && return 1
    sleep 0.1
  done
}

stop_cast() { exec {CAST_FD}>&- 2>/dev/null || true; wait "$CAST_PID" 2>/dev/null || true; }

cast_field() { sed -n "s/.*$1=\([^ ]*\).*/\1/p" "$CAST_OUT" | head -n 1; }

# frames_of NODE COUNT: read COUNT frames from the stream with a GStreamer consumer; prints its
# negotiated caps lines. Exit 3 when no consumer is installed.
frames_of() {
  command -v gst-launch-1.0 >/dev/null 2>&1 || { echo "need gst-launch-1.0 with pipewiresrc" >&2; return 3; }
  timeout 20 gst-launch-1.0 -v pipewiresrc path="$1" num-buffers="$2" ! fakesink 2>&1
}
