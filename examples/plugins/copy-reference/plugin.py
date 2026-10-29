"""Example Pagewing API v1 plugin; Python standard library only."""
import json
from pathlib import Path
import sys

request = json.load(sys.stdin)
if request["api_version"] != 1:
    raise SystemExit("Unsupported Pagewing API")

reference = f"{Path(request['document']).name}, p. {request['page']}"
selection = request["selected_text"].strip()
text = f"{selection}\n\n— {reference}" if selection else reference
json.dump({"copy_text": text, "message": f"Copied reference: {reference}"}, sys.stdout)
