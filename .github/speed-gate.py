"""Fails when any benchmark got slower than the threshold, compared with the saved baseline."""
import json
import pathlib
import sys

root, threshold = pathlib.Path(sys.argv[1]), float(sys.argv[2])
slower = []
for estimates in sorted(root.glob("**/change/estimates.json")):
    name = estimates.parent.parent.relative_to(root)
    change = json.loads(estimates.read_text())["mean"]["point_estimate"]
    print(f"{change:+7.1%}  {name}")
    if change > threshold:
        slower.append(str(name))
if slower:
    sys.exit(f"Slower than {threshold:.0%}: {', '.join(slower)}")
