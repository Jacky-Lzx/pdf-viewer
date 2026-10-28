#!/usr/bin/env python3
"""Check native comment menus, input ownership and explicit status saving."""
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
comments_pdf = runpy.run_path(str(SCRIPTS / "smoke-comments.py"))["comments_pdf"]
TAB, ENTER, UP, DOWN = 0xFF09, 0xFF0D, 0xFF52, 0xFF54


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-comment-menus-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        original = comments_pdf()
        document.write_bytes(original)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        state_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        keyboard.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def resize(size):
                keyboard.x11.XResizeWindow(keyboard.display, window, *size)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)

            def state():
                try:
                    return json.loads(state_path.read_text())
                except (OSError, ValueError):
                    return {}

            def page():
                return (state().get("recent") or [{}])[0].get("page")

            def copied():
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                return keyboard.clipboard()

            def filter_text(value=None):
                keyboard.click((1000, 114))
                if value is not None:
                    keyboard.key("a", ctrl=True)
                    for char in value:
                        keyboard.key(char, delay=0.015)

            def filter_menu(activation=ENTER, burst=False):
                filter_text()
                keyboard.key(TAB)
                if burst:
                    application.send_signal(signal.SIGSTOP)
                    try:
                        _, status = os.waitpid(application.pid, os.WUNTRACED)
                        assert os.WIFSTOPPED(status)
                        keyboard.key(activation, delay=0)
                        keyboard.key("j", shift=True, delay=0)
                        for _ in range(5):
                            keyboard.key(DOWN, delay=0)
                        keyboard.key(ENTER, delay=0)
                        keyboard.x11.XSync(keyboard.display, False)
                    finally:
                        application.send_signal(signal.SIGCONT)
                    time.sleep(0.2)
                else:
                    keyboard.key(activation)

            def report():
                filter_text()
                keyboard.key(TAB, shift=True)  # Filter -> header Copy.
                keyboard.key(ENTER)
                return keyboard.clipboard()

            try:
                window = wait_for(keyboard.focus, "comment menu window")
                wait_for(lambda: page() == 0, "initial page")
                resize((1180, 820))
                keyboard.key("m", ctrl=True, shift=True)
                filter_menu(burst=True)
                exported = report()
                assert "Nested needle jkg" in exported and "[Thread context]" in exported
                assert "Elsewhere comment" not in exported, "Completed filter selects the resolved thread"
                time.sleep(2.2)
                assert page() == 0, "the opening batch belongs to the review menu"
                filter_menu(activation=" ")
                keyboard.key(DOWN)  # All -> None.
                for size in [(240, 240), (1180, 820), (360, 300), (240, 240)]:
                    resize(size)
                keyboard.key(TAB); keyboard.key(TAB, shift=True)
                keyboard.key(ENTER)
                resize((1180, 820))
                exported = report()
                assert "Root question" in exported and "Elsewhere comment" in exported
                assert "Nested needle jkg" not in exported, "resize and Tab retain the None option"
                filter_menu()
                keyboard.key(UP); keyboard.key(DOWN)  # Wrap back to All.
                keyboard.key(ENTER)
                filter_text("elsewhere")
                keyboard.key(ENTER)
                wait_for(lambda: copied() == "Elsewhere comment", "filtered comment editor")
                keyboard.key(TAB)  # Editor -> Status.
                keyboard.key(ENTER); keyboard.key(UP)  # None -> Completed.
                for size in [(240, 240), (1180, 820), (240, 240)]:
                    resize(size)
                keyboard.key(" ")
                assert document.read_bytes() == original, "status selection stays in the draft"
                keyboard.key(ENTER, ctrl=True)
                wait_for(lambda: document.read_bytes() != original, "explicitly saved review status")
                resize((1180, 820))

                def saved_status():
                    exported = report()
                    return "Elsewhere comment" in exported and "Status: Completed / resolved" in exported

                wait_for(saved_status, "saved status survives PDF reload")
                assert page() == 1
                assert state().get("sidebar") is False, "menu Tab must not toggle the sidebar"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: native review filter and editor menus, same-batch opening/arrows/confirmation with reader-key isolation, Arrow/Tab wrap, 240x240 scrolling and live resize, exact filtered exports, explicit status save and PDF reload")
            except Exception:
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
