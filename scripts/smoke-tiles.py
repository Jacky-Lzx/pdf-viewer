#!/usr/bin/env python3
"""Check native detail pixels, zoom fallbacks, panning, rotation, night mode and reload."""
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]
COLORS = [(0.85, 0.15, 0.15), (0.15, 0.70, 0.25), (0.20, 0.30, 0.85), (0.90, 0.70, 0.10)]
UPDATED = [(0.15, 0.75, 0.80), (0.65, 0.20, 0.80), (0.95, 0.40, 0.15), (0.40, 0.45, 0.50)]
POSITIONS = [(420, 260), (760, 260), (420, 580), (760, 580)]


def fixture(colors):
    stream = "\n".join(
        f"{r} {g} {b} rg {x} {y} 300 400 re f"
        for (r, g, b), (x, y) in zip(colors, [(0, 400), (300, 400), (0, 0), (300, 0)])
    )
    # At 800%, each stripe is exactly one display pixel wide. A scaled full-page
    # preview cannot preserve this alternation; visible detail tiles can.
    stream += "\n" + "\n".join(
        f"{index % 2} g {290 + index / 8} 390 0.125 20 re f" for index in range(160)
    )
    data = stream.encode()
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>",
               b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
               b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 4 0 R >>",
               f"<< /Length {len(data)} >>\nstream\n".encode() + data + b"\nendstream"]
    output, offsets = bytearray(b"%PDF-1.4\n"), [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f"{index} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010} 00000 n \n".encode())
    output.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return output


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-tiles-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(fixture(COLORS))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def sequence(value):
                for character in value:
                    keyboard.key(character.lower(), shift=character.isupper() or character in "+$", delay=0.025)

            def pixels(expected, description):
                expected = [tuple(round(value * 255) for value in color) for color in expected]
                def matches():
                    return all(max(abs(a - b) for a, b in zip(keyboard.pixel(pos), color)) < 20
                               for pos, color in zip(POSITIONS, expected))
                wait_for(matches, description)

            def details():
                row = [keyboard.pixel((x, 420))[0] for x in range(560, 610)]
                return sum(abs(a - b) > 200 for a, b in zip(row, row[1:])) >= 40

            try:
                wait_for(keyboard.focus, "detail rendering reader")
                pixels(COLORS, "fit-page quadrants")
                sequence("9999999+")
                pixels(COLORS, "centered 800% quadrants")
                wait_for(details, "native one-pixel stripes from detail tiles", timeout=20)
                sequence("z0")
                pixels([COLORS[0], COLORS[0], COLORS[2], COLORS[2]], "left tiles after panning")
                sequence("z$")
                right = [COLORS[1], COLORS[1], COLORS[3], COLORS[3]]
                pixels(right, "right tiles after panning")
                sequence("i")
                pixels([tuple(1 - value for value in color) for color in right], "night tiles replace daytime textures")
                sequence("r")
                inverted_blue = tuple(1 - value for value in COLORS[2])
                pixels([inverted_blue] * 4, "rotated night tiles use the new orientation")
                sequence("ip")
                order = [2, 0, 3, 1]
                pixels([COLORS[index] for index in order], "full-page fallback after rotation and zoom-out")
                document.write_bytes(fixture(UPDATED))
                keyboard.key("r", ctrl=True)
                pixels([UPDATED[index] for index in order], "reload clears previous document fallback widths")
                assert application.poll() is None
                print("PASS: native one-pixel tile detail, centered zoom, left/right panning, night/rotation replacement, full-page fallback and same-geometry document reload")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                print("Observed pixels:", [keyboard.pixel(pos) for pos in POSITIONS], file=sys.stderr)
                raise
            finally:
                application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
