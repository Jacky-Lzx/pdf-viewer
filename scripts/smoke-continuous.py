#!/usr/bin/env python3
"""Check multi-page drawing, mouse/keyboard scrolling and restored continuous views."""
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def fixture():
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>",
               b"<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R] /Count 3 >>"]
    for index, (width, height, color) in enumerate([
        (600, 800, "0.86 0.24 0.24"),
        (900, 300, "0.20 0.70 0.36"),
        (600, 800, "0.24 0.36 0.86"),
    ]):
        stream = f"{color} rg 0 0 {width} {height} re f".encode()
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Contents {4 + index * 2} 0 R >>".encode())
        objects.append(f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream")
    output, offsets = bytearray(b"%PDF-1.4\n"), [0]
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
    with tempfile.TemporaryDirectory(prefix="pagewing-continuous-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(fixture())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(predicate, description):
                def saved():
                    try:
                        book = json.loads(config.read_text())["recent"][0]
                        return book if predicate(book) else None
                    except (OSError, ValueError, KeyError, IndexError):
                        return None
                return wait_for(saved, description)

            def sequence(text):
                for character in text:
                    keyboard.key(character.lower(), shift=character.isupper(), delay=0.03)

            try:
                wait_for(keyboard.focus, "continuous reader")
                expect(lambda book: book["page"] == 0, "initial page")
                keyboard.key("m", ctrl=True, shift=True)  # Use the full reading canvas.
                sequence("C6j")
                expect(lambda book: book["continuous"] and book["offset"][1] > 0.4,
                       "continuous mode and partial first page")
                wait_for(lambda: keyboard.pixel((600, 150))[0] > 180
                         and keyboard.pixel((600, 650))[1] > 150,
                         "red page tail and green next page are drawn together")
                sequence("gg")
                expect(lambda book: book["page"] == 0 and book["offset"][1] == 0,
                       "first-page command returns to the top even on the same page")
                sequence("15j")
                expect(lambda book: book["page"] > 0, "keyboard crosses page boundaries")
                sequence("999k")
                expect(lambda book: book["page"] == 0 and book["offset"][1] == 0, "top edge clamps")
                keyboard.wheel((800, 500), down=True, steps=60)
                expect(lambda book: book["page"] > 0, "wheel crosses page boundaries")
                sequence("gg6jmaG")
                expect(lambda book: book["page"] == 2, "explicit last page jump")
                sequence("'a")
                first = expect(lambda book: book["page"] == 0 and book["offset"][1] > 0.4,
                               "mark restores the continuous view")
                sequence("C")
                expect(lambda book: not book["continuous"], "single page mode remains available")
                sequence("'a")
                expect(lambda book: book["continuous"] and abs(book["offset"][1] - first["offset"][1]) < 0.001,
                       "mark restores the reading mode as well as its offset")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "restarted continuous reader")
                expect(lambda book: book["continuous"] and book["page"] == 0
                       and abs(book["offset"][1] - first["offset"][1]) < 0.001,
                       "restart restores continuous mode and position")
                keyboard.key("j")
                expect(lambda book: book["offset"][1] > first["offset"][1] + 0.01,
                       "restored document remains scrollable")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: simultaneous mixed-size pages, keyboard/wheel scrolling, edges, marks, mode switching and restart")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config.exists():
                    print(config.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
