#!/usr/bin/env python3
"""Cap and cliclick capture workflow specs."""

from __future__ import annotations

import unittest

from scripts.demo_capture import (
    CAPTURE_WINDOW,
    SHOTS,
    cliclick_commands,
    key_to_cliclick,
    resolve_cap_window_id,
    resolve_shots,
    theme_wants_dark_os,
    titlebar_crop_px,
)
from scripts.demo_session import CAPTURE_WINDOW_TITLE



class DemoCaptureWorkflowTests(unittest.TestCase):
    def test_workspace_shot_focuses_capture_window_without_menu_keys(self) -> None:
        shot = next(row for row in SHOTS if row["name"] == "workspace" and row["theme"] == "day")
        window = {"x": 384, "y": 40, "width": 2160, "height": 1200}
        commands = cliclick_commands(tuple(shot["keys"]), window)
        self.assertEqual(commands, [])
        self.assertNotIn("t:g", commands)
        self.assertNotIn("kd:ctrl", commands)

    def test_groups_shot_opens_all_then_picks_a_group(self) -> None:
        shot = next(row for row in SHOTS if row["name"] == "groups" and row["theme"] == "night")
        commands = cliclick_commands(tuple(shot["keys"]), {"x": 0, "y": 0, "width": 1440, "height": 944})
        self.assertGreaterEqual(sum(1 for item in commands if item.startswith("c:")), 2)
        self.assertFalse(any(item.startswith("kd:") for item in commands))

    def test_agents_shot_opens_all_then_picks_group(self) -> None:
        shot = next(row for row in SHOTS if row["name"] == "agents" and row["theme"] == "night")
        commands = cliclick_commands(tuple(shot["keys"]), {"x": 0, "y": 0, "width": 1440, "height": 944})
        self.assertGreaterEqual(sum(1 for item in commands if item.startswith("c:")), 2)
        self.assertFalse(any(item.startswith("kd:") for item in commands))



    def test_follow_up_shot_right_clicks_an_agent(self) -> None:
        shot = next(row for row in SHOTS if row["name"] == "follow-up" and row["theme"] == "day")
        self.assertNotIn("focus", shot)
        commands = cliclick_commands(
            tuple(shot["keys"]), {"x": 10, "y": 20, "width": 1620, "height": 1008}
        )
        self.assertIn("RightClickCell 6,28", shot["keys"])
        self.assertTrue(any(item.startswith("rc:") for item in commands))
        self.assertFalse(any(item.startswith("kd:") for item in commands))

    def test_workflow_cells_fit_capture_window(self) -> None:
        for shot in SHOTS:
            for key in shot["keys"]:
                kind, _, coords = key.partition(" ")
                if not kind.endswith("Cell"):
                    continue
                col_text, _, row_text = coords.partition(",")
                self.assertGreaterEqual(float(col_text), 0)
                self.assertLess(float(col_text), CAPTURE_WINDOW["columns"])
                self.assertGreaterEqual(float(row_text), 0)
                self.assertLess(float(row_text), CAPTURE_WINDOW["rows"])







    def test_cap_window_id_matches_title_not_size(self) -> None:
        windows = [
            {"id": "1", "name": "~/projects", "bounds": {"width": 2160, "height": 1200}},
            {"id": "103277", "name": CAPTURE_WINDOW_TITLE, "bounds": {"width": 2160, "height": 1200}},
        ]
        self.assertEqual(resolve_cap_window_id(windows, CAPTURE_WINDOW_TITLE), "103277")

    def test_all_resolves_first_cut_shots(self) -> None:
        shots = resolve_shots("all", "all")
        names = [(shot["name"], shot["theme"]) for shot in shots]
        self.assertEqual(
            names,
            [
                ("workspace", "day"),
                ("workspace", "night"),
                ("groups", "day"),
                ("groups", "night"),
                ("agents", "day"),
                ("agents", "night"),
                ("follow-up", "day"),
                ("follow-up", "night"),
                ("collapsed-status", "day"),
                ("collapsed-status", "night"),
                ("commands", "day"),
                ("commands", "night"),
                ("triage", "day"),
                ("triage", "night"),
            ],
        )

    def test_titlebar_crop_matches_retina_png_scale(self) -> None:
        import tempfile
        from pathlib import Path

        from PIL import Image

        with tempfile.TemporaryDirectory() as tmp:
            png = Path(tmp) / "shot.png"
            Image.new("RGB", (2880, 1888), "black").save(png)
            self.assertEqual(titlebar_crop_px(png, png), 64)

    def test_night_shots_request_os_dark_mode(self) -> None:
        self.assertFalse(theme_wants_dark_os("day"))
        self.assertTrue(theme_wants_dark_os("night"))



if __name__ == "__main__":
    unittest.main()
