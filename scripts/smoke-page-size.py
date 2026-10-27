#!/usr/bin/env python3
"""Check huge PDF page sizes and gradual zoom in a real desktop under Xvfb."""
import json
import math
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import time

desktop = runpy.run_path(str(Path(__file__).with_name("smoke-desktop.py")))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def make_pdf():
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>", b"<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R] /Count 3 >>"]
    for page, scale in enumerate([1000, 10**35, 1]):
        stream = f"{scale} 0 0 {scale} 0 0 cm\n0.98 0.97 0.94 rg 0 0 600 800 re f\n0.16 0.45 0.36 rg 30 30 540 740 re f\n0.98 0.97 0.94 rg 40 40 520 720 re f\n0.16 0.45 0.36 rg 250 350 100 100 re f".encode()
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {600 * scale} {800 * scale}] /Contents {4 + 2 * page} 0 R >>".encode())
        objects.append(f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream")
    output = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for index, value in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f"{index} 0 obj\n".encode() + value + b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010} 00000 n \n".encode())
    output.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return output


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-size-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(make_pdf())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(predicate, description):
                def saved():
                    try:
                        book = json.loads(config_path.read_text())["recent"][0]
                        return all(math.isfinite(value) and 0 <= value <= 1 for value in book["offset"]) and predicate(book)
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def custom(book):
                return book["fit"].get("Custom", 0) if isinstance(book["fit"], dict) else 0

            try:
                wait_for(keyboard.focus, "large-page reader window")
                expect(lambda book: book["page"] == 0 and book["fit"] == "Page", "large page loads")
                keyboard.key("+", shift=True)
                expect(lambda book: 0.0005 < custom(book) < 0.002 and 0.49 < book["offset"][1] < 0.51, "gradual zoom from fit-page below five percent")
                keyboard.key("-")
                expect(lambda book: 0 < custom(book) < 0.002 and book["offset"][1] < 0.001, "zoom out fits the huge sheet again")
                keyboard.key("w"); keyboard.key("+", shift=True)
                expect(lambda book: 0.002 < custom(book) < 0.003, "fit-width uses the huge page dimensions")
                keyboard.key("r"); keyboard.key("p"); keyboard.key("+", shift=True)
                expect(lambda book: book["rotation"] == 1 and 0 < custom(book) < 0.005, "rotated huge sheet fits and zooms")
                keyboard.key("j", shift=True); keyboard.key("p")
                keyboard.key("+", shift=True)
                expect(lambda book: book["page"] == 1 and 0 < custom(book) < 1e-30, "near-float-limit page remains usable")
                for _ in range(70):
                    keyboard.key("+", shift=True, delay=0.015)
                expect(lambda book: book["page"] == 1 and 1e-31 < custom(book) < 2.1e-31, "maximum zoom bounds UI coordinates and saves finite offsets")
                keyboard.key("p"); keyboard.key("a")
                expect(lambda book: book["page"] == 1 and 1e-31 < custom(book) < 2.1e-31,
                       "100% request still obeys extreme page coordinate limits")
                keyboard.key("g", shift=True); keyboard.key("r", shift=True); keyboard.key("p")
                keyboard.key("+", shift=True)
                expect(lambda book: book["page"] == 2 and book["rotation"] == 0 and 0.8 < custom(book) < 1.4, "normal pages retain their usual zoom")
                keyboard.key("a")
                expect(lambda book: book["page"] == 2 and custom(book) == 1,
                       "100% request is exact for ordinary page dimensions")
                assert application.poll() is None, "reader must stay running"
                print("PASS: large and near-float-limit page fitting, gradual zoom, rotation, bounded 100% requests and normal-page recovery")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                application.terminate()
                application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
