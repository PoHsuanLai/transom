# Sourceable: provides `require_jail` by sourcing the desktop harness's harness/lib/jail.sh and
# bus.sh, found as a sibling checkout: $TRANSOM_DESKTOP, then <repo>/../desktop, then
# $HOME/desktop. Missing: prints the fix and exits 3.
_transom_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
_transom_desktop=""
for _d in "${TRANSOM_DESKTOP:-}" "$_transom_root/../desktop" "$HOME/desktop"; do
  if [ -n "$_d" ] && [ -f "$_d/harness/lib/jail.sh" ]; then _transom_desktop=$_d; break; fi
done
if [ -z "$_transom_desktop" ]; then
  echo "dev/lib/jail.sh: the desktop harness (harness/lib/jail.sh) was not found" >&2
  echo "fix: check out the desktop repository beside transom, or set TRANSOM_DESKTOP" >&2
  exit 3
fi
# shellcheck source=/dev/null
source "$_transom_desktop/harness/lib/jail.sh"
# shellcheck source=/dev/null
source "$_transom_desktop/harness/lib/bus.sh"
unset _d
