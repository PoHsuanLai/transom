#!/usr/bin/env python3
"""A small client for the jailed chain's acceptance checks (stdlib plus PyGObject's Gio).

  portal_client.py fake-shell LOG ANSWER [ANSWER...]
      Own org.quire.Shell and serve Shell1.PickCaptureSource, Shell1.PickColor (a fixed colour,
      or a cancel for `none`) and Shell1.AskPermission (the answer in TRANSOM_FAKE_ACCESS: 0
      granted, the default; 1 denied; 2 dismissed). Every call is appended to LOG as
      one line; the answer is the sources given, each `output:NAME` or `window:IDENTIFIER`, with
      the pointer mode from TRANSOM_FAKE_CURSOR (default: the first mode the app allows). `none` answers with a cancel.
      Runs until killed.

  portal_client.py cast TYPES PERSIST [RESTORE_TOKEN]
      Register as the app org.quire.CastCheck, then run ScreenCast CreateSession, SelectSources and
      Start through the portal frontend (a share is remembered only for an app with an id). TYPES is
      the portal bit set (1 display, 2 window), PERSIST the persist mode. Prints one line:
      `response=N node=ID token=TOKEN size=WxH`; the node and token are empty when absent.
      The session stays open until the process is killed or stdin closes.

  portal_client.py screenshot
      Run Screenshot.Screenshot through the portal frontend and wait for its answer, staying on the
      bus meanwhile (the frontend closes a request whose caller has left). Prints one line:
      `response=N uri=URI`; the uri is empty when absent.

Exit 3 when PyGObject is missing.
"""
import sys

try:
    import gi
    gi.require_version("Gio", "2.0")
    from gi.repository import Gio, GLib
except (ImportError, ValueError):
    print("portal_client.py: PyGObject (Gio) is not installed", file=sys.stderr)
    sys.exit(3)

SHELL_XML = """<node><interface name="org.quire.Shell1">
  <method name="PickCaptureSource">
    <arg type="s" name="app_label" direction="in"/><arg type="u" name="kinds" direction="in"/>
    <arg type="u" name="multiple" direction="in"/><arg type="au" name="cursor_modes" direction="in"/>
    <arg type="a(ss)" name="sources" direction="out"/><arg type="u" name="cursor" direction="out"/>
  </method>
  <method name="PickColor">
    <arg type="s" name="app_label" direction="in"/>
    <arg type="a(ddd)" name="color" direction="out"/>
  </method>
  <method name="AskPermission">
    <arg type="s" name="app_label" direction="in"/><arg type="s" name="title" direction="in"/>
    <arg type="s" name="subtitle" direction="in"/><arg type="s" name="body" direction="in"/>
    <arg type="s" name="grant_label" direction="in"/><arg type="s" name="deny_label" direction="in"/>
    <arg type="u" name="answer" direction="out"/>
  </method></interface></node>"""

PORTAL = "org.freedesktop.portal.Desktop"
PORTAL_PATH = "/org/freedesktop/portal/desktop"


def fake_shell(log_path, answers):
    import os
    sources = [] if answers == ["none"] else [tuple(a.split(":", 1)) for a in answers]
    cursor = os.environ.get("TRANSOM_FAKE_CURSOR")
    access = int(os.environ.get("TRANSOM_FAKE_ACCESS", "0"))

    def on_call(conn, sender, path, iface, method, params, invocation):
        if method == "PickColor":
            (label,) = params.unpack()
            with open(log_path, "a") as log:
                log.write(f"color label={label!r}\n")
            colour = [] if answers == ["none"] else [(0.25, 0.5, 0.75)]
            invocation.return_value(GLib.Variant("(a(ddd))", (colour,)))
            return
        if method == "AskPermission":
            label, title, subtitle, body, grant, deny = params.unpack()
            with open(log_path, "a") as log:
                log.write(f"ask label={label!r} title={title!r} grant={grant!r} deny={deny!r}\n")
            invocation.return_value(GLib.Variant("(u)", (access,)))
            return
        label, kinds, multiple, modes = params.unpack()
        with open(log_path, "a") as log:
            log.write(f"pick label={label!r} kinds={kinds} multiple={multiple} modes={modes}\n")
        # The person picks one of the modes the app allows, the first unless told otherwise.
        mode = int(cursor) if cursor else (modes[0] if modes else 1)
        invocation.return_value(GLib.Variant("(a(ss)u)", (sources, mode)))

    def on_bus(conn, name):
        node = Gio.DBusNodeInfo.new_for_xml(SHELL_XML)
        conn.register_object("/org/quire/Shell1", node.interfaces[0], on_call, None, None)

    Gio.bus_own_name(Gio.BusType.SESSION, "org.quire.Shell", Gio.BusNameOwnerFlags.NONE,
                     on_bus, None, None)
    GLib.MainLoop().run()


# The app id the cast check registers as. A restore grant is kept only for an app with an
# identity (PORTAL-16), and a host app has one only once it registers it with the portal.
CAST_APP_ID = "org.quire.CastCheck"


def register(conn, app_id):
    """Name this connection's app to the portal frontend, as a host app (GTK, Qt) does before any
    other portal call: org.freedesktop.host.portal.Registry.Register (xdg-desktop-portal 1.19)."""
    conn.call_sync(PORTAL, PORTAL_PATH, "org.freedesktop.host.portal.Registry", "Register",
                   GLib.Variant("(sa{sv})", (app_id, {})), None, Gio.DBusCallFlags.NONE, 10000,
                   None)


def cast(types, persist, token):
    conn = Gio.bus_get_sync(Gio.BusType.SESSION)
    register(conn, CAST_APP_ID)
    sender = conn.get_unique_name()[1:].replace(".", "_")
    loop = GLib.MainLoop()
    counter = [0]

    def request(method, args_sig, args, options):
        counter[0] += 1
        handle_token = f"check{counter[0]}"
        options = dict(options, handle_token=GLib.Variant("s", handle_token))
        path = f"{PORTAL_PATH}/request/{sender}/{handle_token}"
        got = {}

        def on_response(_c, _s, _p, _i, _n, params):
            got["value"] = params.unpack()
            loop.quit()

        sub = conn.signal_subscribe(PORTAL, "org.freedesktop.portal.Request", "Response", path,
                                    None, Gio.DBusSignalFlags.NONE, on_response)
        body = GLib.Variant(f"({args_sig}a{{sv}})", (*args, options))
        conn.call_sync(PORTAL, PORTAL_PATH, "org.freedesktop.portal.ScreenCast", method, body,
                       None, Gio.DBusCallFlags.NONE, 60000, None)
        GLib.timeout_add_seconds(60, loop.quit)
        loop.run()
        conn.signal_unsubscribe(sub)
        return got.get("value", (2, {}))

    code, results = request("CreateSession", "", (), {
        "session_handle_token": GLib.Variant("s", "checksession")})
    session = results.get("session_handle")
    if code != 0 or not session:
        print(f"response={code or 2} node= token= size=")
        return
    options = {"types": GLib.Variant("u", types), "persist_mode": GLib.Variant("u", persist),
               "cursor_mode": GLib.Variant("u", 2)}
    if token:
        # The app-facing API takes the token as a string; the portal frontend swaps it for the
        # backend's own restore data (a (suv)) and back.
        options["restore_token"] = GLib.Variant("s", token)
    code, _ = request("SelectSources", "o", (session,), options)
    if code != 0:
        print(f"response={code} node= token= size=")
        return
    code, results = request("Start", "os", (session, ""), {})
    streams = results.get("streams", [])
    node = streams[0][0] if streams else ""
    size = streams[0][1].get("size") if streams else None
    text = results.get("restore_token", "")
    print(f"response={code} node={node} token={text} "
          f"size={'x'.join(map(str, size)) if size else ''}", flush=True)
    sys.stdin.read()


def screenshot():
    conn = Gio.bus_get_sync(Gio.BusType.SESSION)
    sender = conn.get_unique_name()[1:].replace(".", "_")
    loop = GLib.MainLoop()
    handle_token = "accept"
    path = f"{PORTAL_PATH}/request/{sender}/{handle_token}"
    got = {}

    def on_response(_c, _s, _p, _i, _n, params):
        got["value"] = params.unpack()
        loop.quit()

    sub = conn.signal_subscribe(PORTAL, "org.freedesktop.portal.Request", "Response", path,
                                None, Gio.DBusSignalFlags.NONE, on_response)
    options = {"handle_token": GLib.Variant("s", handle_token)}
    body = GLib.Variant("(sa{sv})", ("", options))
    conn.call_sync(PORTAL, PORTAL_PATH, "org.freedesktop.portal.Screenshot", "Screenshot", body,
                   None, Gio.DBusCallFlags.NONE, 60000, None)
    GLib.timeout_add_seconds(60, loop.quit)
    loop.run()
    conn.signal_unsubscribe(sub)
    code, results = got.get("value", (2, {}))
    print(f"response={code} uri={results.get('uri', '')}", flush=True)


if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["fake-shell"] and len(args) >= 3:
        fake_shell(args[1], args[2:])
    elif args[:1] == ["cast"] and len(args) >= 3:
        cast(int(args[1]), int(args[2]), args[3] if len(args) > 3 else "")
    elif args == ["screenshot"]:
        screenshot()
    else:
        print(__doc__, file=sys.stderr)
        sys.exit(64)
