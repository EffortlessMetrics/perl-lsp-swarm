"""Check the blocking status in the hand-maintained CI references."""
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def blocking_rows(filename, id_column):
    header = []
    for line in (ROOT / "docs/ci" / filename).read_text().splitlines():
        if not line.startswith("|"):
            header = []
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if not header:
            header = cells
        elif id_column in header and "Blocking?" in header:
            lane = cells[header.index(id_column)]
            if not lane.strip("-:") or (id_column == "Whitelist id" and lane.startswith("_")):
                continue
            yield lane.strip("`"), cells[header.index("Blocking?")].strip("*")


class LaneReferenceTests(unittest.TestCase):
    def test_documented_blocking_status(self):
        whitelist = tomllib.loads((ROOT / "policy/ci-lane-whitelist.toml").read_text())
        blocking = {row["id"]: row["blocking"] for row in whitelist["lane"]}
        planner = tomllib.loads((ROOT / "policy/ci-lanes.toml").read_text())["lane"]
        for filename, column in (("ci-lane-map.md", "Lane id"), ("inventory.md", "Whitelist id")):
            rows = list(blocking_rows(filename, column))
            self.assertIn("pr_smoke", dict(rows), filename)
            for lane, status in rows:
                with self.subTest(filename=filename, lane=lane):
                    self.assertEqual(status, "yes" if blocking[lane] else "no")
                    if lane in planner:
                        self.assertEqual(planner[lane]["blocking"], blocking[lane])
