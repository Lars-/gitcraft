"""The speed check for pull requests.

`record <criterion dir> <out.json> [--reverse]` stores how much slower each benchmark of the pull
request was than the base branch: criterion compares against the run saved before, which is the
base (or, with `--reverse`, the pull request). `judge <threshold> <round>...` fails for a benchmark
that was slower than the threshold in every round. Measuring in both orders cancels out a runner
that speeds up or slows down while the job runs.
"""
import json
import pathlib
import sys


def record(root, out, reverse=False):
    slower = {}
    for estimates in sorted(pathlib.Path(root).glob("**/change/estimates.json")):
        change = json.loads(estimates.read_text())["mean"]["point_estimate"]
        name = str(estimates.parent.parent.relative_to(root))
        slower[name] = 1 / (1 + change) - 1 if reverse else change
    pathlib.Path(out).write_text(json.dumps(slower))


def judge(threshold, *rounds):
    rounds = [json.loads(pathlib.Path(r).read_text()) for r in rounds]
    failed = []
    for name in sorted(rounds[0]):
        changes = [r.get(name, 0.0) for r in rounds]
        print("  ".join(f"{c:+7.1%}" for c in changes), name)
        if min(changes) > threshold:
            failed.append(name)
    if failed:
        sys.exit(f"Slower than {threshold:.0%} in every round: {', '.join(failed)}")


if __name__ == "__main__":
    command, *args = sys.argv[1:]
    if command == "record":
        record(args[0], args[1], "--reverse" in args)
    else:
        judge(float(args[0]), *args[1:])
