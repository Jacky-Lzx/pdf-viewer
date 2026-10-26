#!/usr/bin/env python3
"""Copy visible web-link hints with F in real native windows, without opening URLs."""
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
make_pdf = runpy.run_path(str(SCRIPTS / "make-sample.py"))["make_pdf"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-link-copy-") as directory:
        root = Path(directory)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        keyboard = Keyboard()
        application = None
        with open(root / "application.log", "w+") as log:
            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page, rotation=0):
                wait_for(lambda: book().get("page") == page and book().get("rotation") == rotation, f"page {page + 1}, rotation {rotation}")

            def sequence(value, burst=False):
                if burst:
                    application.send_signal(signal.SIGSTOP)
                try:
                    for character in value:
                        keyboard.key(character.lower(), shift=character.isupper(), delay=0 if burst else 0.025)
                finally:
                    if burst:
                        application.send_signal(signal.SIGCONT)

            def home():
                sequence("gg")
                expect(0)
                time.sleep(0.25)

            def copied(value):
                wait_for(lambda: keyboard.clipboard() == value, f"copied link {value}")

            def launch(name, data):
                nonlocal application
                document = root / name / "folio-smoke.pdf"
                document.parent.mkdir()
                document.write_bytes(data)
                application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "link-copy reader window")
                expect(0)
                time.sleep(0.4)
                return application

            try:
                application = launch("dense", make_pdf(hint_links=1000, web_hints=True))
                keyboard.key("y")
                sequence("Fbm")
                keyboard.key(0xFF08)
                sequence("ml")
                copied("https://example.com/link/999")
                expect(0)
                sequence("99FaabJ", burst=True)
                copied("https://example.com/link/1")
                expect(1)
                sequence("FJ", burst=True)
                expect(2)
                home()
                sequence("FbmzJ", burst=True)
                expect(1)
                home()
                sequence("Fb")
                keyboard.key(0xFF1B)
                sequence("J")
                expect(1)
                home()
                sequence("r")
                expect(0, 1)
                time.sleep(0.25)
                sequence("Fbml", burst=True)
                copied("https://example.com/link/999")
                expect(0, 1)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                application = launch("mixed", (SCRIPTS.parent / "assets/folio-guide.pdf").read_bytes())
                sequence("Fa", burst=True)
                copied("https://www.rust-lang.org/")
                expect(0)
                sequence("fa", burst=True)
                expect(1)
                home()
                sequence("FaJ", burst=True)
                copied("https://www.rust-lang.org/")
                expect(1)
                keyboard.key("f", ctrl=True)
                sequence("Fa")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                copied("Fa")
                keyboard.key(0xFF1B)
                sequence("J")
                expect(2)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 1000 web-link copy hints, exact URLs, Backspace, same-frame copying/count consumption, no-link fallback, invalid prefixes, Escape, rotated links, mixed internal/web filtering, normal f navigation and search input isolation")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application is not None and application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
