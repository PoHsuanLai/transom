#!/bin/sh
# Install transom: the binary, its systemd user unit, its D-Bus activation file and its portal file.
#
#   scripts/install.sh [--prefix DIR] [--destdir DIR] [--bin-dir DIR] [--no-build]
#
# --prefix    where the software goes (default /usr/local; the environment variable PREFIX also sets it)
# --destdir   a staging root put in front of every path, for packagers and for trying the install
#             (default none; the environment variable DESTDIR also sets it)
# --bin-dir   install the already built transom found in DIR instead of building it
# --no-build  install from target/release (or $CARGO_TARGET_DIR/release) without building first
#
# What goes where (DESTDIR, when given, is in front of all of it):
#   PREFIX/libexec/transom                                                 the portal backend
#   PREFIX/lib/systemd/user/transom.service                                the user unit
#   PREFIX/share/dbus-1/services/org.freedesktop.impl.portal.desktop.quire.service   D-Bus activation
#   PREFIX/share/xdg-desktop-portal/portals/quire.portal                   which interfaces it answers, UseIn=quire
#
# Not installed: the portals.conf routing (dist/quire-portals.conf is the example). Which backend
# answers which interface is the desktop's choice, so the desktop's installer ships that file. The unit
# is not enabled (that writes under the home directory): the first portal call starts transom through
# D-Bus activation.
#
# The script is standalone: it knows nothing of any desktop. Run again, it changes nothing that is
# already as it should be. It writes nothing under a home directory, and only writes under /etc or
# /usr when run, without a destdir, by someone who may.
set -eu

prefix=${PREFIX:-/usr/local}
destdir=${DESTDIR:-}
bin_dir=
build=yes

usage() {
    sed -n '2,/^set -eu/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case $1 in
        --prefix) prefix=${2:?--prefix needs a directory}; shift 2 ;;
        --prefix=*) prefix=${1#--prefix=}; shift ;;
        --destdir) destdir=${2:?--destdir needs a directory}; shift 2 ;;
        --destdir=*) destdir=${1#--destdir=}; shift ;;
        --bin-dir) bin_dir=${2:?--bin-dir needs a directory}; shift 2 ;;
        --bin-dir=*) bin_dir=${1#--bin-dir=}; shift ;;
        --no-build) build=no; shift ;;
        -h | --help) usage; exit 0 ;;
        *) echo "install.sh: unknown option $1 (try --help)" >&2; exit 2 ;;
    esac
done

case $prefix in
    /*) ;;
    *) echo "install.sh: the prefix must be an absolute path, not '$prefix'" >&2; exit 2 ;;
esac
prefix=${prefix%/}
destdir=${destdir%/}

root=$(cd "$(dirname "$0")/.." && pwd)
dist=$root/dist
changed=0

mode_of() {
    stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1"
}

# put MODE SOURCE TARGET [FROM TO]: write SOURCE to DESTDIR/TARGET with mode MODE, replacing the
# text FROM by TO on the way when given. An existing target with the same content is left alone.
put() {
    mode=$1
    source=$2
    target=$destdir$3
    mkdir -p "$(dirname "$target")"
    staged=$target.new.$$
    if [ $# -ge 5 ]; then
        FROM=$4 TO=$5 awk '
            BEGIN { from = ENVIRON["FROM"]; to = ENVIRON["TO"]; n = length(from) }
            {
                out = ""; line = $0
                while ((i = index(line, from)) > 0) {
                    out = out substr(line, 1, i - 1) to
                    line = substr(line, i + n)
                }
                print out line
            }' "$source" >"$staged"
    else
        cp "$source" "$staged"
    fi
    chmod "$mode" "$staged"
    if [ -f "$target" ] && cmp -s "$staged" "$target" && [ "$(mode_of "$target")" = "$(mode_of "$staged")" ]; then
        rm -f "$staged"
    else
        mv -f "$staged" "$target"
        changed=$((changed + 1))
        echo "installed $3"
    fi
}

if [ -n "$bin_dir" ]; then
    built=$bin_dir
else
    if [ "$build" = yes ]; then
        (cd "$root" && cargo build --release --locked -p transom)
    fi
    built=${CARGO_TARGET_DIR:-$root/target}/release
fi
if [ ! -x "$built/transom" ]; then
    echo "install.sh: $built/transom is not there; build first or point --bin-dir at it" >&2
    exit 1
fi

# The files in dist/ name /usr/libexec/transom; a different prefix changes that one path.
libexec=$prefix/libexec
put 755 "$built/transom" "$libexec/transom"
put 644 "$dist/transom.service" "$prefix/lib/systemd/user/transom.service" /usr/libexec/transom "$libexec/transom"
put 644 "$dist/org.freedesktop.impl.portal.desktop.quire.service" \
    "$prefix/share/dbus-1/services/org.freedesktop.impl.portal.desktop.quire.service" /usr/libexec/transom "$libexec/transom"
put 644 "$dist/quire.portal" "$prefix/share/xdg-desktop-portal/portals/quire.portal"

if [ "$changed" -eq 0 ]; then
    echo "nothing to change"
fi
