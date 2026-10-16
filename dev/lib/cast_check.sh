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

# stop_cast: end the cast run_cast started. Safe to call when none was started (an EXIT trap runs
# it even when the script stopped before its first cast, with CAST_FD and CAST_PID unset).
stop_cast() {
  if [ -n "${CAST_FD:-}" ]; then
    exec {CAST_FD}>&-
    CAST_FD=""
  fi
  if [ -n "${CAST_PID:-}" ]; then
    wait "$CAST_PID" 2>/dev/null || true
    CAST_PID=""
  fi
}

# stop_fake_shell: stop the shell start_fake_shell started, if any. Never signals pid 0, which
# would be this script's own process group.
stop_fake_shell() {
  if [ -n "${SHELL_PID:-}" ]; then
    kill "$SHELL_PID" 2>/dev/null || true
    wait "$SHELL_PID" 2>/dev/null || true
    SHELL_PID=""
  fi
}

cast_field() { sed -n "s/.*$1=\([^ ]*\).*/\1/p" "$CAST_OUT" | head -n 1; }

# frames_of NODE COUNT: read COUNT frames from the stream with a GStreamer consumer; prints its
# negotiated caps lines. Exit 3 when no consumer is installed.
frames_of() {
  command -v gst-launch-1.0 >/dev/null 2>&1 || { echo "need gst-launch-1.0 with pipewiresrc" >&2; return 3; }
  # The consumer says it takes video, as a screen recorder does: WirePlumber links only nodes of
  # one media type, and a bare pipewiresrc is "Stream/Input/Unknown", so it never finds the share;
  # the caps filter gives PipeWire formats to offer (ANY caps offer none, and the link fails).
  timeout 20 gst-launch-1.0 -v pipewiresrc path="$1" num-buffers="$2" \
    stream-properties="props,media.type=Video,media.category=Capture,media.role=Screen" ! video/x-raw ! fakesink 2>&1
}

# explain_node NODE: after a consumer could not find NODE, print what PipeWire and the session
# manager know of it: the graph object (class, name, object.serial, ports), what WirePlumber
# logged for the id, and whether the consumer finds the node by object serial
# (`target-object=<serial>`) when `path=<id>` did not. Diagnosis only; it never changes a verdict.
explain_node() {
  local node=$1 dump serial code log
  command -v pw-dump >/dev/null 2>&1 || { echo "need pw-dump to explain the node"; return 0; }
  dump=$(pw-dump 2>&1) || true
  read -r -d '' code <<'PY' || true
import json, sys
node = int(sys.argv[1])
objects = json.load(sys.stdin)

def props_of(o):
    return (o.get("info") or {}).get("props") or {}

found = [o for o in objects if o.get("id") == node]
if not found:
    print(f"no object {node} in the graph; video nodes there:")
    for o in objects:
        p = props_of(o)
        if o.get("type", "").endswith("Node") and p.get("media.class", "").startswith("Video/"):
            print(f"  id={o['id']} serial={p.get('object.serial')} name={p.get('node.name')}")
    sys.exit(0)
obj = found[0]
p = props_of(obj)
ports = [o for o in objects if o.get("type", "").endswith("Port") and props_of(o).get("node.id") == node]
print(f"node {node}: class={p.get('media.class')} name={p.get('node.name')} "
      f"serial={p.get('object.serial')} state={obj['info'].get('state')} "
      f"ports={len(ports)} driver={p.get('node.driver')}")
if len(sys.argv) > 2:
    print(p.get("object.serial", ""), file=open(sys.argv[2], "w"))
PY
  serial=$(transom_state_dir)/node-serial
  python3 -I -c "$code" "$node" "$serial" <<<"$dump" || true
  serial=$(cat "$serial" 2>/dev/null || true)
  log=$(transom_state_dir)/wireplumber.log
  if [ -f "$log" ]; then
    echo "wireplumber, lines naming node $node:"
    grep -E "[^0-9]$node[^0-9]" "$log" | tail -n 10 | sed 's/^/  /' || true
  fi
  if [ -n "$serial" ] && command -v gst-launch-1.0 >/dev/null 2>&1; then
    if timeout 10 gst-launch-1.0 pipewiresrc target-object="$serial" num-buffers=1 \
        stream-properties="props,media.type=Video,media.category=Capture,media.role=Screen" ! video/x-raw ! fakesink >/dev/null 2>&1; then
      echo "target-object=$serial (the object serial) is found; path=$node is not"
    else
      echo "target-object=$serial (the object serial) is not found either"
    fi
  fi
}
