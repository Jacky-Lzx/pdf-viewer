#!/usr/bin/env python3
"""Stop and restart a real long-document search, retaining a stable clipboard snapshot."""
import ctypes
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
PAGES = 8000
TAB, ENTER = 0xFF09, 0xFF0D


def long_pdf():
    # Shared content keeps the PDF small, but every page requires a real text
    # extraction. One hit per page stays below the 10,000-result cap.
    stream = (b"BT /F1 10 Tf 40 760 Td (Needle) Tj\n"
              + b"0 -8 Td (Padding text for the long document search workload.) Tj\n" * 100
              + b"ET")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        f"<< /Type /Pages /Count {PAGES} /Kids [".encode()
        + b" ".join(f"{page + 5} 0 R".encode() for page in range(PAGES)) + b"] >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream",
    ]
    objects.extend([b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] "
                    b"/Resources << /Font << /F1 3 0 R >> >> /Contents 4 0 R >>"] * PAGES)
    data = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for number, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f"{number} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(data)
    data.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        data.extend(f"{offset:010} 00000 n \n".encode())
    data.extend(f"trailer\n<< /Root 1 0 R /Size {len(offsets)} >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return bytes(data)


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-search-stop-") as directory:
        root = Path(directory)
        document = root / "long-search.pdf"
        document.write_bytes(long_pdf())
        config = root / "config/pagewing/library.json"
        config.parent.mkdir(parents=True)
        config.write_text(json.dumps({"sidebar": False}))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def reading():
                try:
                    return json.loads(config.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return None

            def copy_from_run(stopped_expected=True):
                keyboard.key(TAB, shift=True)
                keyboard.key(TAB, shift=True)
                keyboard.key(ENTER)
                def copied():
                    report = keyboard.clipboard()
                    stopped = "[Search stopped by user;" in report
                    return report if report.startswith("Pagewing search snippets\n") and stopped == stopped_expected else None
                return wait_for(copied, "clipboard snapshot reflects the current search state")

            def count(report):
                return int(next(line.split(": ", 1)[1] for line in report.splitlines()
                                if line.startswith("Available results: ")))

            try:
                window = wait_for(lambda: keyboard.focus(document.name), "long-search reader window")
                initial = wait_for(reading, "initial saved reading state")
                keyboard.key("f", ctrl=True)
                for character in "Needle":
                    keyboard.key(character.lower(), shift=character.isupper(), delay=0.02)
                keyboard.key(ENTER)  # Explicit query also enables History.
                keyboard.key("f", ctrl=True)  # Confirming a query returns focus to reading.
                for _ in range(8):
                    keyboard.key(TAB, delay=0.04)
                keyboard.key(ENTER)  # Stop, leaving all accepted hits in place.
                stopped = copy_from_run()
                assert "[Search stopped by user;" in stopped, stopped[:500]
                assert 0 < count(stopped) < PAGES, stopped[:500]
                assert "Query: Needle\n" in stopped
                assert "[Search complete.]" not in stopped
                time.sleep(0.6)
                keyboard.key(ENTER)  # Copy again; queued old batches must be ignored.
                assert keyboard.clipboard() == stopped, "stopped results must remain stable"
                for _ in range(2):
                    keyboard.key(TAB)
                keyboard.key(ENTER)  # Restart from the same control.
                time.sleep(0.6)
                restarted = copy_from_run(stopped_expected=False)
                assert "Query: Needle\n" in restarted and count(restarted) > 0
                assert "[Search stopped by user;" not in restarted
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                for _ in range(2):
                    keyboard.key(TAB)
                keyboard.key(0x20)  # Stop with Space after horizontal focus reveal.
                compact = copy_from_run()
                assert "[Search stopped by user;" in compact, compact[:500]
                current = reading()
                for field in ["page", "fit", "rotation", "continuous", "facing", "offset"]:
                    assert current.get(field) == initial.get(field), (field, initial, current)
                print("PASS: 8,000-page native search Stop/Restart, stable partial snapshots, explicit clipboard status, keyboard Enter/Space, 240-pixel focus reveal and preserved reading view")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
