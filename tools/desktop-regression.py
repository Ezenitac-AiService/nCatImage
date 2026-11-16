#!/usr/bin/env python3
"""Isolated X11 key/mouse, drop, mock network, cancellation and privacy checks."""
import os
import subprocess
import time

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, GLib, Gtk
import pyatspi

from smoke_support import arguments, check_privacy, fixtures, image_command, logs, work_directory


def main():
    args = arguments(__doc__)
    if os.environ.get("NCAT_PRIVATE_TEST_BUS") != "1" or os.environ.get("GDK_BACKEND") != "x11":
        raise SystemExit("Use Xvfb + dbus-run-session with NCAT_PRIVATE_TEST_BUS=1 and GDK_BACKEND=x11; never use your desktop bus")
    Gtk.init()
    context = GLib.MainContext.default()
    work = work_directory("ncat-desktop-")
    inputs = fixtures(work)
    context_deadline = 8

    def pump(seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            while context.pending():
                context.iteration(False)
            time.sleep(.005)

    def wait(check, label, seconds=context_deadline):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            pump(.04)
            try:
                result = check()
                if result:
                    return result
            except (subprocess.CalledProcessError, IndexError, RuntimeError):
                pass
        raise AssertionError(label)

    def command(*argv):
        return subprocess.check_output(argv, text=True, stderr=subprocess.DEVNULL).strip()

    def key(name):
        subprocess.run(["xdotool", "key", "--clearmodifiers", name], check=True)
        pump(.13)

    def nodes(node=None):
        node = pyatspi.Registry.getDesktop(0) if node is None else node
        yield node
        for child in node:
            if child:
                yield from nodes(child)

    def showing(role):
        return [n for n in nodes() if n.getRoleName() == role and n.getState().contains(pyatspi.STATE_SHOWING)]

    def window():
        return wait(lambda: command("xdotool", "search", "--onlyvisible", "--name", "nCatImage").splitlines()[-1], "viewer")

    def activate(win):
        subprocess.run(["xdotool", "windowactivate", "--sync", win], check=True)

    def click(win, x, y):
        # Fixed Ubuntu GTK/Adwaita 1000x800 fixture. GTK 4.14's AT-SPI screen
        # origins can be zero, so use screenshot-verified window coordinates.
        subprocess.run(["xdotool", "mousemove", "--window", win, str(x), str(y), "click", "1"], check=True)
        pump(.13)

    def click_spin(win):
        spin = showing("spin button")[0]
        controls = spin.parent
        group_width = controls.queryComponent().getExtents(pyatspi.DESKTOP_COORDS).width
        previous_width = controls[0].queryComponent().getExtents(pyatspi.DESKTOP_COORDS).width
        geometry = dict(line.split("=", 1) for line in command("xdotool", "getwindowgeometry", "--shell", win).splitlines())
        # Sizes work even where GTK's screen origins do not. Account for CJK
        # fallback font widths rather than hardcoding the spin's x position.
        x = (int(geometry["WIDTH"]) - group_width) // 2 + previous_width + 8 + 20
        click(win, x, 100)

    def launch(label, uri=None, smoke=False):
        directory = work / label
        directory.mkdir()
        argv = (["--smoke-test"] if smoke else []) + ([str(uri)] if uri else [])
        return subprocess.Popen(image_command(args.image, *argv), cwd=directory), directory

    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    reply = bus.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName",
                          GLib.Variant("(su)", ("org.kde.KIOFuse", 4)), GLib.VariantType.new("(u)"),
                          Gio.DBusCallFlags.NONE, 2000, None)
    assert reply.unpack() == (1,), "KIOFuse already owns this bus; refusing to replace it"
    xml = '<node><interface name="org.kde.KIOFuse.VFS"><method name="mountUrl"><arg type="s" direction="in"/><arg type="s" direction="out"/></method></interface></node>'
    pending = []

    def on_call(connection, sender, path, interface, method, params, invocation):
        uri = params.unpack()[0]
        assert uri.split(":", 1)[0] in ["smb", "ftp", "sftp"]
        def respond():
            invocation.return_value(GLib.Variant("(s)", (str(inputs["book"]),)))
            return GLib.SOURCE_REMOVE
        if "/hang" in uri:
            pending.append(invocation)
        elif "/delay" in uri:
            GLib.timeout_add(2500, respond)
        else:
            respond()

    registration = bus.register_object("/org/kde/KIOFuse", Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0], on_call, None, None)
    wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    app = None
    source = None
    try:
        wait(lambda: "window id #" in command("xprop", "-root", "_NET_SUPPORTING_WM_CHECK"),
             "window manager readiness")
        app, directory = launch("ui", inputs["book"])
        win = window()
        activate(win)
        def page(number):
            wait(lambda: command("xdotool", "getwindowname", win).startswith(f"{number} / 3"), f"page {number}")
        page(1)
        wait(lambda: showing("spin button"), "accessible controls")
        for press, target in [("Down", 2), ("Up", 1), ("space", 2), ("BackSpace", 1), ("Next", 2), ("Prior", 1), ("Tab", 1), ("Down", 2), ("Up", 1)]:
            key(press)
            page(target)
        key("d")
        for press, target in [("Down", 2), ("Up", 1), ("Left", 2), ("Right", 1)]:
            key(press)
            page(target)
        click_spin(win)
        key("ctrl+a")
        key("2")
        key("Return")
        page(2)
        key("Up")
        page(3)
        click(win, 500, 450)
        key("Up")
        page(2)
        normal_height = showing("scroll pane")[0].queryComponent().getExtents(pyatspi.DESKTOP_COORDS).height
        key("grave")
        wait(lambda: not showing("spin button"), "hide interface")
        assert showing("scroll pane")[0].queryComponent().getExtents(pyatspi.DESKTOP_COORDS).height > normal_height
        key("grave")
        wait(lambda: showing("spin button"), "restore interface")
        key("F11")
        wait(lambda: not showing("spin button"), "fullscreen")
        key("grave")
        assert not showing("spin button")
        key("F11")
        wait(lambda: showing("spin button"), "windowed restore")
        key("grave")
        key("F11")
        key("F11")
        assert not showing("spin button")
        key("grave")
        wait(lambda: showing("spin button"), "show interface")
        click_spin(win)
        key("Escape")
        wait(lambda: app.poll() is not None, "exit while editing")
        assert app.returncode == 0
        print("PASS keyboard, explicit focus, top UI visibility and fullscreen restoration")

        source = Gtk.Window(title="NCAT_DRAG_SOURCE")
        source.set_default_size(250, 200)
        source.set_child(Gtk.Label(label="Drag synthetic fixture"))
        drag = Gtk.DragSource()
        drag.set_actions(Gdk.DragAction.COPY)
        payload = None
        drag.connect("prepare", lambda *unused: payload)
        source.add_controller(drag)
        source.present()
        source_id = wait(lambda: command("xdotool", "search", "--onlyvisible", "--name", "^NCAT_DRAG_SOURCE$").splitlines()[-1], "drag source")
        subprocess.run(["xdotool", "windowmove", source_id, "20", "30"], check=True)
        cases = [
            ("uri", "text/uri-list", [inputs["book"].as_uri()], "Displayed"),
            ("list", "list", [inputs["book"].as_uri()], "Displayed"),
            ("folder", "text/uri-list", [inputs["folder"].as_uri()], "Displayed"),
            ("multiple", "text/uri-list", [inputs["book"].as_uri(), inputs["folder"].as_uri()], "MultipleItems"),
            ("remote", "text/uri-list", ["smb://DO-NOT-LOG.invalid/book.cbz"], "Displayed"),
            ("unsupported-uri", "text/uri-list", ["https://DO-NOT-LOG.invalid/book.cbz"], "NonLocal"),
            ("invalid", "text/uri-list", [inputs["bad"].as_uri()], "OpenFailed"),
            ("unsupported-text", "text/plain", ["DO-NOT-LOG secret"], "Entered"),
        ]
        for label, kind, uris, expected in cases:
            if kind == "list":
                payload = Gdk.ContentProvider.new_for_value(Gdk.FileList.new_from_array([Gio.File.new_for_uri(uri) for uri in uris]))
            else:
                payload = Gdk.ContentProvider.new_for_bytes(kind, GLib.Bytes.new(("\r\n".join(uris) + "\r\n").encode()))
            app, directory = launch("drop-" + label)
            win = window()
            subprocess.run(["xdotool", "windowsize", win, "800", "700", "windowmove", win, "400", "200"], check=True)
            pump(.25)
            move = subprocess.Popen(["xdotool", "windowactivate", "--sync", source_id, "mousemove", "100", "120", "mousedown", "1", "sleep", ".2", "mousemove", "115", "135", "sleep", ".2", "mousemove", "270", "180", "sleep", ".2", "mousemove", "500", "350", "sleep", ".3", "mousemove", "750", "550", "sleep", ".4", "mouseup", "1"])
            wait(lambda: move.poll() is not None, "drag input")
            wait(lambda: expected in logs(directory), label + " outcome")
            pump(.1)
            text = logs(directory)
            check_privacy(text)
            if expected == "Displayed":
                assert "Received" in text and "OpenQueued" in text
            elif expected == "OpenFailed":
                assert "OpenFailed" in text
            else:
                assert "Displayed" not in text and "OpenQueued" not in text
            activate(win)
            key("Escape")
            wait(lambda: app.poll() is not None, "drop exit")
            print("PASS drag/drop and redaction:", label)
        source.destroy()
        source = None

        for protocol in ["smb", "ftp", "sftp"]:
            app, directory = launch(protocol, f"{protocol}://DO-NOT-LOG.invalid/book.cbz", smoke=True)
            wait(lambda: app.poll() is not None, protocol + " open")
            assert app.returncode == 0
            text = logs(directory)
            check_privacy(text)
            assert "NetworkResolved" in text and "bridge: KioFuse" in text
            print("PASS mocked OS bridge:", protocol)

        app, directory = launch("cancel-stale", "smb://DO-NOT-LOG.invalid/delay")
        wait(lambda: "NetworkResolving" in logs(directory), "resolving")
        win = window()
        activate(win)
        key("ctrl+l")
        wait(lambda: command("xdotool", "search", "--onlyvisible", "--name", "네트워크 파일 또는 폴더 열기"), "network dialog")
        subprocess.run(["xdotool", "type", "--clearmodifiers", "--delay", "1", inputs["last"].as_uri()], check=True)
        key("Return")
        page(3)
        pump(2.6)
        page(3)
        text = logs(directory)
        check_privacy(text)
        assert "NetworkCancelled" in text and "NetworkResolved" not in text
        activate(win)
        key("Escape")
        wait(lambda: app.poll() is not None, "cancel exit")
        print("PASS network entry, cancellation and stale-result suppression")

        app, directory = launch("close-busy", "sftp://DO-NOT-LOG.invalid/hang")
        wait(lambda: "NetworkResolving" in logs(directory), "busy mount")
        win = window()
        activate(win)
        key("F11")
        key("Escape")
        wait(lambda: app.poll() is not None, "nonblocking fullscreen exit", seconds=3)
        assert "NetworkCancelled" in logs(directory)
        check_privacy(logs(directory))
        print("PASS fullscreen exit while remote service has not replied")
    finally:
        if app and app.poll() is None:
            app.terminate()
        if source:
            source.destroy()
        wm.terminate()
        bus.unregister_object(registration)
    print(f"Synthetic inputs and results retained in {work}")


if __name__ == "__main__":
    main()
