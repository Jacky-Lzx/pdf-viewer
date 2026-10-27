#!/usr/bin/env python3
"""Cancel disconnected/stalled FIFOs without extra input, then read another PDF."""
import errno
import json
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


def run_case(binary, connected):
    with tempfile.TemporaryDirectory(prefix="pagewing-load-cancel-") as directory:
        root = Path(directory)
        slow, document = root / "unfinished.pdf", root / "pagewing-smoke.pdf"
        os.mkfifo(slow)
        document.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
        config = root / "config/pagewing/library.json"
        config.parent.mkdir(parents=True)
        config.write_text(json.dumps({"dark": False, "sidebar": False,
                                      "recent": [{"path": str(document), "page": 2}]}))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        writer = None
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(slow)], env=environment,
                                           stdout=log, stderr=log)

            def connect_writer():
                try:
                    return os.open(slow, os.O_WRONLY | os.O_NONBLOCK)
                except OSError as error:
                    if error.errno == errno.ENXIO:
                        return None
                    raise

            def saved_page(page):
                try:
                    return any(book["path"] == str(document) and book["page"] == page
                               for book in json.loads(config.read_text())["recent"])
                except (OSError, ValueError, KeyError):
                    return False

            try:
                wait_for(lambda: keyboard.focus("unfinished.pdf"), "slow document window")
                if connected:
                    writer = wait_for(connect_writer, "background FIFO reader")
                    os.write(writer, b"%")
                time.sleep(0.1)
                assert keyboard.pixel((350, 100))[0] < 245, "slow input must still be loading"
                keyboard.key("w", ctrl=True)
                keyboard.key("p", ctrl=True)
                for character in "pagewing-smoke":
                    keyboard.key(character, delay=0)
                keyboard.key(0xFF0D)
                wait_for(keyboard.focus, "replacement PDF selected")

                # Send no more bytes and never signal EOF. Neither a blocking
                # open nor a blocking block read can reach the replacement PDF.
                wait_for(lambda: keyboard.pixel((350, 100))[0] > 245,
                         "replacement PDF visible while old stream has no EOF")
                keyboard.key("y")
                assert "Make yourself at home." in keyboard.clipboard(), "restore the new PDF's saved page"
                keyboard.key("j", shift=True)
                wait_for(lambda: saved_page(3), "Vim navigation after cancelled load")
                keyboard.key("y")
                assert "A thought worth keeping." in keyboard.clipboard()
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                state = "one byte with writer still open" if connected else "no writer connected"
                print(f"PASS: cancel FIFO ({state}), open saved PDF without extra input or EOF, "
                      "exact page text, Vim navigation and graceful quit")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config.exists():
                    print(config.read_text(), file=sys.stderr)
                raise
            finally:
                if writer is not None:
                    os.close(writer)
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve()
    for connected in (False, True):
        run_case(binary, connected)
