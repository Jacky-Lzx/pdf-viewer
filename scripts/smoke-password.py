#!/usr/bin/env python3
"""Exercise keyboard-only unlock/retry/reload using a real encrypted PDF."""
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


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-password-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        fixture = runpy.run_path(str(SCRIPTS / "make-encrypted.py"))["make_pdf"]()
        assert fixture == (SCRIPTS.parent / "tests/fixtures/encrypted.pdf").read_bytes()
        document.write_bytes(fixture)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        book = json.loads(config_path.read_text())["recent"][0]
                        return book["page"] == page and book["rotation"] == 0 and book["fit"] == "Page"
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def password(value, submit=True):
                application.send_signal(signal.SIGSTOP)
                try:
                    for character in value:
                        keyboard.key(character, delay=0)
                    if submit:
                        keyboard.key(0xFF0D, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.8)

            def copied_page(page):
                keyboard.key("y")
                assert f"Encrypted document page {page + 1}." in keyboard.clipboard()

            try:
                window = wait_for(keyboard.focus, "encrypted document reader window")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.8)
                keyboard.key(0xFF1B)
                keyboard.key("r", ctrl=True)
                time.sleep(0.8)
                password("wrong")
                assert not config_path.exists(), "Wrong password unexpectedly opened the document"
                password("jkgdi", submit=False)
                keyboard.key(0xFF09); keyboard.key(" ")
                keyboard.key(0xFF09, shift=True)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "jkgdi", "Show password reveals the typed value without losing it"
                keyboard.key(0xFF0D)
                expect(0, "correct password retries without clicking the field")
                copied_page(0)
                keyboard.key("j", shift=True)
                expect(1, "unlocked PDF supports keyboard navigation")
                # Reload while search owns focus: the password must go to its
                # own field, while retaining the previous document query.
                keyboard.key("f", ctrl=True)
                for character in "Rust":
                    keyboard.key(character.lower(), shift=character.isupper())
                keyboard.key("r", ctrl=True)
                time.sleep(0.8)
                password("wrong")
                password("jkgdi")
                copied_page(1)
                expect(1, "encrypted reload retains the current page after retry")
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "Rust", "Password input leaked into the document query"
                keyboard.key(0xFF1B)
                keyboard.key("r", ctrl=True)
                time.sleep(0.8)
                # Input, Show password, Unlock, Cancel: Enter must activate Cancel.
                for _ in range(3):
                    keyboard.key(0xFF09)
                keyboard.key(0xFF0D)
                keyboard.key("j", shift=True)
                expect(2, "cancelling reload password keeps the active PDF readable")
                copied_page(2)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert "jkgdi" not in config_path.read_text()
                config = json.loads(config_path.read_text())
                assert not config["dark"] and not config["night"] and not config["thumbnails"]
                application = subprocess.Popen([str(binary), "--page", "2", str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "encrypted start-page window")
                time.sleep(0.8)
                password("wrong")
                password("jkgdi")
                copied_page(1)
                expect(1, "explicit start page survives password retries and overrides saved page")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert "jkgdi" not in config_path.read_text()
                print("PASS: minimum-size keyboard-only unlock, cancel/reopen before first unlock, show/hide password value, wrong-password focus, decrypted text, reload/search/Cancel recovery, explicit start page after retries and no password in saved state")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.send_signal(signal.SIGCONT)
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
