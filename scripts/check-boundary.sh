#!/usr/bin/env bash
# transom's boundary rows, mechanically. ARCHITECTURE.md section 1 is the table; this is its
# mechanical form. Rows:
#   T1 transom-core reaches no zbus, tokio, pipewire, wayland, image or tracing
#   T2 only crates/transom/src/capture/ names the wayland crates
#   T3 only crates/transom/src/screencast/pipewire.rs names the pipewire crate (a path through transom's own `pipewire` module, `self::pipewire::` or `screencast::pipewire::`, is not the crate)
#   T4 only crates/transom/src/screenshot.rs (P2-T1) names the image crate
#   T5 no clock, no environment read and no file I/O in transom-core
#   T6 the environment is read only in crates/transom/src/main.rs
#   T7 no unsafe anywhere
#   T8 no other portal backend's name in code or comments (they are facts at most, never sources)
#   T9 no bool fields or parameters in transom-core's public items
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0
bad() { echo "LEAK: $*"; fail=1; }
ok() { echo "holds: $*"; }

reached() {
  cargo tree -p "$1" -e normal --prefix none --format '{p}' 2>/dev/null | awk '{print $1}' | sort -u
}

# T1: a check on OUTPUT (cargo tree exits non-zero in states we do not mind).
tree=$(reached transom-core)
if [ -z "$tree" ]; then
  echo "ERROR: cargo tree found nothing for transom-core; the boundary was not checked"
  exit 1
fi
found=$(printf '%s\n' "$tree" | grep -E '^(zbus[a-z_-]*|zvariant[a-z_-]*|tokio[a-z_-]*|pipewire[a-z_-]*|libspa[a-z_-]*|wayland-[a-z-]+|image|tracing[a-z_-]*|async-[a-z-]+)$' | tr '\n' ' ')
if [ -n "$found" ]; then bad "T1 transom-core reaches ${found% }"; else ok "T1 transom-core is pure"; fi

# names_only <label> <extended regex> <allowed path regex> [roots...]: the pattern appears in
# Rust code only under paths matching the allowed regex. Comment lines are not code.
names_only() {
  local label="$1" pattern="$2" allowed="$3"; shift 3
  local roots=("$@") hits
  [ ${#roots[@]} -eq 0 ] && roots=(crates)
  hits=$(grep -rnE --include='*.rs' "$pattern" "${roots[@]}" 2>/dev/null \
    | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' | grep -vE "^($allowed)" || true)
  if [ -n "$hits" ]; then bad "$label"; printf '%s\n' "$hits" | head -20; else ok "$label"; fi
}

names_only "T2 only capture/ names the wayland crates" '\bwayland_(client|protocols|backend)\b' 'crates/transom/src/capture/'
names_only "T3 only screencast/pipewire.rs names pipewire" '(^|[^:[:alnum:]_])(pipewire|libspa)::' 'crates/transom/src/screencast/pipewire\.rs'
names_only "T4 only screenshot.rs names image" '\bimage::' 'crates/transom/src/screenshot\.rs'
names_only "T5 transom-core reads no clock, environment or file" \
  '\b(Instant::now|SystemTime::now|std::env::|env::var|std::fs::|File::open|File::create)\b' '$^' crates/transom-core
names_only "T6 only main.rs reads the environment" '\b(std::env::|env::var)' 'crates/transom/src/main\.rs' crates/transom/src
names_only "T7 no unsafe" '\bunsafe\b' '$^'

# T8 is checked on comments too: another backend is a fact to read about, never code to copy.
hits=$(grep -rniE --include='*.rs' 'xdg-desktop-portal-(wlr|gnome|kde|cosmic|luminous|hyprland|gtk)|xdpw' crates 2>/dev/null || true)
if [ -n "$hits" ]; then bad "T8 another backend is named"; printf '%s\n' "$hits" | head; else ok "T8 no other backend named"; fi

# T9: a `bool` in transom-core's public signatures or fields (a predicate's return type is fine).
hits=$(grep -rnE --include='*.rs' 'pub [a-z_]+: bool|[(,][[:space:]]*[a-z_]+: bool[,)]' crates/transom-core/src 2>/dev/null || true)
if [ -n "$hits" ]; then bad "T9 a bool field or parameter in transom-core"; printf '%s\n' "$hits" | head; else ok "T9 no bool fields or parameters in transom-core"; fi

exit $fail
