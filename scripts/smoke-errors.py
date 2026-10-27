#!/usr/bin/env python3
"""Copy full diagnostics and dismiss independent error panels in a small X11 window."""
import ctypes
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def run(binary):
    for dark in [False, True]:
        with tempfile.TemporaryDirectory(prefix="pagewing-errors-") as directory:
            root = Path(directory)
            document = root / "pagewing-smoke.pdf"
            document.write_bytes((SCRIPTS.parent / "tests/fixtures/encrypted.pdf").read_bytes()
                                 .replace(b"/V 1", b"/V 9"))
            preferences = root / "config/pagewing/config.toml"
            preferences.parent.mkdir(parents=True)
            invalid_config = '[appearance]\ndark = "bad value"\n'
            preferences.write_text(invalid_config)
            environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                               XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
            keyboard = Keyboard()
            keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                                   ctypes.c_uint, ctypes.c_uint]
            with (root / "application.log").open("w+") as log:
                application = subprocess.Popen([str(binary), str(document)], env=environment,
                                               stdout=log, stderr=log)
                try:
                    window = wait_for(keyboard.focus, "error recovery reader window")
                    keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                    keyboard.x11.XFlush(keyboard.display)
                    time.sleep(0.8)
                    if dark:
                        keyboard.key("d")
                    # Two panels share the remaining viewport. The configuration
                    # diagnostic is much longer than its scrolling message area.
                    keyboard.click((167, 122))
                    expected = "Cannot open PDF: this encryption method is not supported."
                    assert keyboard.clipboard() == expected
                    keyboard.click((107, 192))
                    config_error = keyboard.clipboard()
                    assert str(preferences) in config_error
                    assert 'invalid type: string "bad value", expected a boolean' in config_error
                    assert config_error.endswith("Using built-in defaults.")
                    # Clear the isolated display's clipboard so an unchanged
                    # previous value cannot disguise a missed Space activation.
                    keyboard.x11.XSetSelectionOwner.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                                                ctypes.c_ulong, ctypes.c_ulong]
                    clipboard = keyboard.x11.XInternAtom(keyboard.display, b"CLIPBOARD", 0)
                    keyboard.x11.XSetSelectionOwner(keyboard.display, clipboard, 0, 0)
                    keyboard.x11.XFlush(keyboard.display)
                    keyboard.key(" ")
                    assert keyboard.clipboard() == config_error, "Space must activate the focused Copy button"
                    keyboard.click((40, 192))
                    time.sleep(0.3)
                    # Dismissing configuration keeps the document error, whose
                    # actions now move down into the freed space.
                    keyboard.click((167, 190))
                    assert keyboard.clipboard() == expected
                    assert preferences.read_text() == invalid_config
                    keyboard.key("q", ctrl=True)
                    assert application.wait(timeout=8) == 0
                except Exception:
                    log.flush(); log.seek(0)
                    print(log.read(), file=sys.stderr)
                    raise
                finally:
                    if application.poll() is None:
                        application.terminate(); application.wait(timeout=5)
                    keyboard.x11.XCloseDisplay(keyboard.display)
    print("PASS: minimum-size light/dark error panels, full document/configuration diagnostic clipboard, "
          "Space activation, independent dismissal and unchanged invalid preferences")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
