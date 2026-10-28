#!/usr/bin/env python3
"""Check keyboard View menu selection, compact scrolling and input ownership."""
import ctypes
import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]
TAB, ENTER, ESCAPE, UP, DOWN = 0xFF09, 0xFF0D, 0xFF1B, 0xFF52, 0xFF54


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-view-menu-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        original = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
        document.write_bytes(original)
        config = root / "config/pagewing"
        config.mkdir(parents=True)
        (config / "config.toml").write_text('[display]\nfit = 2.0\n')
        state_path = config / "library.json"
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        keyboard.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def state():
                try:
                    return json.loads(state_path.read_text())
                except (OSError, ValueError):
                    return {}

            def book():
                return (state().get("recent") or [{}])[0]

            def expect(predicate, description):
                wait_for(lambda: book().get("page") == 2 and predicate(book()), description)

            def resize(size):
                keyboard.x11.XResizeWindow(keyboard.display, window, *size)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)

            def open_view(activation=ENTER, burst=False, selection=None):
                keyboard.key("f", ctrl=True)
                # Copy/History are disabled for this unconfirmed, absent query.
                # Aa, Word, Prev, Next, Results, Close, the compact search
                # scrollbar and bottom drag viewport, then Page, +, Zoom, -, View.
                for _ in range(13):
                    keyboard.key(TAB)
                if burst or selection is not None:
                    application.send_signal(signal.SIGSTOP)
                    try:
                        _, status = os.waitpid(application.pid, os.WUNTRACED)
                        assert os.WIFSTOPPED(status)
                        keyboard.key(activation, delay=0)
                        if selection is None:
                            keyboard.key("j", shift=True, delay=0)
                        else:
                            for _ in range(selection):
                                keyboard.key(DOWN, delay=0)
                            keyboard.key(ENTER, delay=0)
                        keyboard.x11.XSync(keyboard.display, False)
                    finally:
                        application.send_signal(signal.SIGCONT)
                else:
                    keyboard.key(activation)

            def choose(index):
                for _ in range(index):
                    keyboard.key(DOWN)
                keyboard.key(ENTER)

            try:
                window = wait_for(keyboard.focus, "View menu reader")
                wait_for(lambda: bool(book().get("path")), "initial document")
                keyboard.key("3"); keyboard.key("g", shift=True)
                expect(lambda value: value.get("fit") == {"Custom": 2.0}, "starting view")
                keyboard.key("f", ctrl=True)
                for char in "menu draft":
                    keyboard.key(char, delay=0.02)
                resize((240, 240))
                open_view(burst=True)
                time.sleep(2.2)
                assert book()["page"] == 2, "menu opening must own later reader keys"
                choose(1)
                expect(lambda value: value["continuous"] and not value["facing"], "keyboard Continuous")
                open_view(activation=" ")
                choose(5)
                expect(lambda value: value["fit"] == "Height", "last compact fit choices are reachable")
                open_view()
                keyboard.key(UP)
                for size in [(1180, 820), (240, 240), (360, 300), (240, 240)]:
                    resize(size)
                keyboard.key(TAB)
                keyboard.key(TAB, shift=True)
                keyboard.key(" ")
                expect(lambda value: value["fit"] == {"Custom": 1.0}, "resize retains the last option; Tab wraps inside the menu")
                open_view(); choose(2)
                expect(lambda value: value["facing"] and not value["continuous"], "keyboard Two pages")
                open_view(); choose(4)
                expect(lambda value: value["fit"] == "Width", "keyboard Fit width")
                open_view(); choose(3)
                expect(lambda value: value["fit"] == "Page", "keyboard Fit page")
                open_view(); choose(0)
                expect(lambda value: not value["facing"] and not value["continuous"], "keyboard Single page")
                open_view(selection=2)
                expect(lambda value: value["facing"] and not value["continuous"], "same-batch menu opening, arrows and confirmation")
                open_view(activation=" ", selection=0)
                expect(lambda value: not value["facing"] and not value["continuous"], "same-batch Space opening and Enter confirmation")
                open_view(); keyboard.key(ESCAPE)
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "menu draft", "menu activation and Escape retain the search draft"
                assert state()["sidebar"] is False, "menu Tab must not toggle the sidebar"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert document.read_bytes() == original
                print("PASS: all seven View choices via keyboard, first-item focus, 240x240 scrolling, Arrow/Tab wrap, live resize focus, same-batch opening/arrows/confirmation, opening/reader-key ownership, Escape, preserved draft/page/sidebar and unchanged PDF")
            except Exception:
                print(f"Process status: {application.poll()!r}", file=sys.stderr)
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if state_path.exists():
                    print(state_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.send_signal(signal.SIGCONT)
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
