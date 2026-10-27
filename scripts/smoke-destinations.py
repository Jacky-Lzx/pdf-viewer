#!/usr/bin/env python3
"""Exercise PDF target positions, view modes and same-page jumps under Xvfb."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-destinations-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(make_pdf(destination_views={1: "/XYZ 48 500 2", 2: "/FitR 100 100 300 300", 3: "/FitH 600", 4: "/FitV 250", 5: "/XYZ null 650 null"}))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(predicate, description):
                def saved():
                    try:
                        return predicate(json.loads(config_path.read_text())["recent"][0])
                    except (OSError, ValueError, KeyError, IndexError, TypeError):
                        return False
                wait_for(saved, description)

            def custom(book):
                return book["fit"].get("Custom", 0) if isinstance(book["fit"], dict) else 0

            def burst(value):
                application.send_signal(signal.SIGSTOP)
                try:
                    for letter in value:
                        keyboard.key(letter.lower(), shift=letter.isupper() or letter == "$", delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)

            try:
                wait_for(keyboard.focus, "destination reader window")
                expect(lambda book: book["page"] == 0, "initial page")
                burst("famb")
                expect(lambda book: book["page"] == 1 and custom(book) == 2
                       and book["marks"].get("b", {}).get("page") == 1
                       and 0.6 < book["marks"]["b"]["offset"][1] < 0.7,
                       "same-frame link and mark save the resolved target position")
                keyboard.key("g"); keyboard.key("g"); keyboard.key("p")
                keyboard.key("'"); keyboard.key("b")
                expect(lambda book: book["page"] == 1 and custom(book) == 2 and 0.6 < book["offset"][1] < 0.7,
                       "saved same-frame destination mark restores its actual location")
                keyboard.key("g"); keyboard.key("g"); keyboard.key("p")
                burst("faJ")
                expect(lambda book: book["page"] == 2 and custom(book) == 2,
                       "same-frame link then page jump reaches the next page")
                keyboard.key(0xFF51, alt=True)
                expect(lambda book: book["page"] == 1 and custom(book) == 2 and 0.6 < book["offset"][1] < 0.7,
                       "history retains the resolved same-frame PDF target")
                keyboard.key(0xFF53, alt=True)
                expect(lambda book: book["page"] == 2 and custom(book) == 2 and book["offset"][1] == 0,
                       "forward history restores the later page after the batched jump")
                keyboard.key("g"); keyboard.key("g"); keyboard.key("p")
                keyboard.key("f"); keyboard.key("a")
                expect(lambda book: book["page"] == 1 and custom(book) == 2 and 0.6 < book["offset"][1] < 0.7, "Vim link hints apply XYZ target zoom and location")
                keyboard.key("g"); keyboard.key("g"); keyboard.key("p")
                keyboard.click((460, 626))
                expect(lambda book: book["page"] == 1 and custom(book) == 2 and 0.6 < book["offset"][1] < 0.7, "mouse link uses the same target location")
                keyboard.key(0xFF09); time.sleep(0.3)
                keyboard.key("1"); keyboard.key("0"); keyboard.key("0"); keyboard.key("k")
                expect(lambda book: book["page"] == 1 and book["offset"][1] == 0, "move away from the target on its own page")
                keyboard.click((100, 237))
                expect(lambda book: book["page"] == 1 and 0.6 < book["offset"][1] < 0.7, "same-page outline action repositions the viewport")
                keyboard.key(0xFF51, alt=True)
                expect(lambda book: book["page"] == 1 and custom(book) == 2 and book["offset"][1] == 0, "same-page back restores the exact reading position")
                keyboard.key(0xFF53, alt=True)
                expect(lambda book: book["page"] == 1 and custom(book) == 2 and 0.6 < book["offset"][1] < 0.7, "same-page forward restores the target view")
                keyboard.key("r"); keyboard.click((100, 237))
                expect(lambda book: book["page"] == 1 and book["rotation"] == 1 and 0.15 < book["offset"][1] < 0.25, "XYZ coordinates follow reading rotation")
                keyboard.key("r", shift=True)
                keyboard.click((100, 272))
                expect(lambda book: book["page"] == 2 and 3 < custom(book) < 4 and 0.80 < book["offset"][1] < 0.87, "named rectangular destination fits and centers its region")
                keyboard.click((100, 377))
                expect(lambda book: book["page"] == 5 and 3 < custom(book) < 4 and 0.23 < book["offset"][1] < 0.27, "nullable XYZ destination preserves zoom and sets the requested top")
                keyboard.key("r"); keyboard.key("p")
                expect(lambda book: book["page"] == 5 and book["rotation"] == 1 and book["fit"] == "Page", "change the view before returning")
                keyboard.key(0xFF51, alt=True)
                expect(lambda book: book["page"] == 2 and book["rotation"] == 0 and 3 < custom(book) < 4 and 0.80 < book["offset"][1] < 0.87, "cross-page history restores zoom rotation and scroll")
                keyboard.key(0xFF53, alt=True)
                expect(lambda book: book["page"] == 5 and book["rotation"] == 1 and book["fit"] == "Page", "forward history restores the view where back was pressed")
                keyboard.key("r", shift=True)
                keyboard.click((100, 307))
                expect(lambda book: book["page"] == 3 and book["fit"] == "Width" and 0.5 < book["offset"][1] < 0.65, "named FitH destination applies width fit and top coordinate")
                keyboard.click((100, 342))
                expect(lambda book: book["page"] == 4 and book["fit"] == "Height" and book["offset"][1] == 0, "FitV destination fits page height")
                keyboard.key("g"); keyboard.key("g"); keyboard.key("p")
                time.sleep(0.2)
                keyboard.click((100, 377))
                expect(lambda book: book["page"] == 5 and 0.8 < custom(book) < 0.9, "nullable target zoom retains the actual scale from an automatic fit mode")
                keyboard.key("2"); keyboard.key(0xFF51, alt=True)
                expect(lambda book: book["page"] == 4 and book["fit"] == "Height", "counted history back restores automatic height fit")
                keyboard.key("2"); keyboard.key(0xFF53, alt=True)
                expect(lambda book: book["page"] == 5 and 0.8 < custom(book) < 0.9, "counted history forward restores two reading positions")
                kids = b"/Kids [5 0 R 7 0 R 9 0 R 11 0 R 13 0 R 15 0 R]"
                document.write_bytes(document.read_bytes().replace(kids, b"/Kids [5 0 R 7 0 R 9 0 R]".ljust(len(kids), b" "), 1).replace(b"/Count 6", b"/Count 3", 1))
                keyboard.key("r", ctrl=True)
                expect(lambda book: book["page"] == 2, "reload clamps the current page before using view history")
                keyboard.key(0xFF51, alt=True)
                expect(lambda book: book["page"] == 0, "reload retains valid previous views")
                keyboard.key(0xFF53, alt=True)
                expect(lambda book: book["page"] == 2, "reload prunes missing pages from forward view history")
                keyboard.key(0xFF09)
                burst("ggp")
                expect(lambda book: book["page"] == 0 and book["fit"] == "Page", "prepare same-frame destination alignment")
                time.sleep(0.2)
                burst("fazt")
                expect(lambda book: book["page"] == 1 and custom(book) == 2
                       and book["offset"][0] > 0.99 and book["offset"][1] == 0,
                       "same-frame PDF target and zt retain the target horizontal position")
                burst("ggp")
                expect(lambda book: book["page"] == 0 and book["fit"] == "Page", "prepare same-frame destination bottom alignment")
                time.sleep(0.2)
                burst("fazb")
                expect(lambda book: book["page"] == 1 and custom(book) == 2
                       and book["offset"][0] > 0.99 and book["offset"][1] == 1,
                       "same-frame PDF target and zb retain the target horizontal position")
                keyboard.key(0xFF51, alt=True)
                expect(lambda book: book["page"] == 0 and book["fit"] == "Page", "alignment preserves the existing history entry")
                keyboard.key(0xFF53, alt=True)
                expect(lambda book: book["page"] == 1 and book["offset"] == [1, 1], "history restores the aligned PDF target")
                for command, horizontal in [("z0", 0), ("z$", 1)]:
                    burst("ggp")
                    expect(lambda book: book["page"] == 0 and book["fit"] == "Page", "prepare horizontal destination alignment")
                    time.sleep(0.2)
                    burst("fa" + command)
                    expect(lambda book: book["page"] == 1 and custom(book) == 2
                           and book["offset"][0] == horizontal and 0.65 < book["offset"][1] < 0.67,
                           "same-frame horizontal alignment preserves PDF target vertical position")
                print("PASS: PDF destinations, same-frame link/mark/history and vertical alignment, same/cross-page view history, zoom/rotation/scroll restoration, counts and reload pruning")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                application.send_signal(signal.SIGCONT)
                application.terminate()
                application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
