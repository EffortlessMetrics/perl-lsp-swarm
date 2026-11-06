"""Reject repository product dependencies in the selection normal/build graph."""
import json
from pathlib import Path
import sys


def selection_graph(metadata):
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    roots = [p["id"] for p in packages.values() if p["name"] == "perl-ci-hygiene"]
    if len(roots) != 1:
        raise ValueError("expected one selection owner")
    reached = set()
    pending = roots[:]
    while pending:
        current = pending.pop()
        if current in reached:
            continue
        reached.add(current)
        for dep in nodes[current]["deps"]:
            if any(kind["kind"] in (None, "build") for kind in dep["dep_kinds"]):
                pending.append(dep["pkg"])
    forbidden = [packages[p]["name"] for p in reached
                 if packages[p]["source"] is None and p not in roots]
    if forbidden:
        raise ValueError("selection graph includes repository dependencies: " + ", ".join(sorted(forbidden)))
    return reached


if __name__ == "__main__":
    ids = selection_graph(json.loads(Path(sys.argv[1]).read_text()))
    print(f"selection normal/build graph: {len(ids)} package IDs; only repository owner perl-ci-hygiene")
