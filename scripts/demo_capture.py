#!/usr/bin/env python3
"""Capture Gardn marketing shots with Cap CLI and cliclick."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts.demo_session import (
    CAPTURE_WINDOW,
    CAPTURE_WINDOW_TITLE,
    GHOSTTY_APP,
    REPO_ROOT,
    apply_theme,
    close_capture_window,
    default_bin,
    default_home,
    focus_capture_process,
    focus_named_workspace_tab,
    focus_showcase,
    install_fixture,
    is_capture_window,
    list_ghostty_windows,
    open_capture_window,
    prepare_demo_runtime,
    report_demo_states,
    restore_other_ghostty,
    seed,
    session_path,
    start_server,
    stop_server,
)



def cell_px(col: float, row: float) -> tuple[int, int]:
    width = CAPTURE_WINDOW["width"] / CAPTURE_WINDOW["columns"]
    height = CAPTURE_WINDOW["height"] / CAPTURE_WINDOW["rows"]
    return (int(col * width + width / 2), int(row * height + height / 2))


def pointer(kind: str, col: float, row: float) -> tuple[str, ...]:
    if kind == "Move":
        return (f"MoveCell {col},{row}",)
    return (f"MoveCell {col},{row}", "Wait 500", f"{kind}Cell {col},{row}")




THEMES = ("day", "night")

SHOTS = (
    {"name": "workspace", "theme": "day", "video": True, "keys": ()},
    {"name": "workspace", "theme": "night", "video": True, "keys": ()},
    {
        "name": "groups",
        "theme": "day",
        "video": True,
        "keys": pointer("Click", 26, 0)
        + ("Wait 500",)
        + pointer("Move", 26, 3)
        + pointer("Click", 31, 6)
        + ("Wait 900",),
    },
    {
        "name": "groups",
        "theme": "night",
        "video": True,
        "keys": pointer("Click", 26, 0)
        + ("Wait 500",)
        + pointer("Move", 26, 3)
        + pointer("Click", 31, 6)
        + ("Wait 900",),
    },
    {
        "name": "agents",
        "theme": "day",
        "video": True,
        "keys": pointer("Click", 26, 23)
        + ("Wait 500",)
        + pointer("Move", 26, 26)
        + pointer("Click", 31, 28)
        + ("Wait 900",),
    },
    {
        "name": "agents",
        "theme": "night",
        "video": True,
        "keys": pointer("Click", 26, 23)
        + ("Wait 500",)
        + pointer("Move", 26, 26)
        + pointer("Click", 31, 28)
        + ("Wait 900",),
    },
    {
        "name": "follow-up",
        "theme": "day",
        "video": True,
        "keys": pointer("RightClick", 8, 24) + ("Wait 1200",),
    },
    {
        "name": "follow-up",
        "theme": "night",
        "video": True,
        "keys": pointer("RightClick", 8, 24) + ("Wait 1200",),
    },
    {
        "name": "collapsed-status",
        "theme": "day",
        "video": True,
        "keys": pointer("Click", 30, CAPTURE_WINDOW["rows"] - 2)
        + ("Wait 700",)
        + pointer("Move", 1.5, 3.5)
        + ("Wait 900",),
    },
    {
        "name": "collapsed-status",
        "theme": "night",
        "video": True,
        "keys": pointer("Click", 30, CAPTURE_WINDOW["rows"] - 2)
        + ("Wait 700",)
        + pointer("Move", 1.5, 3.5)
        + ("Wait 900",),
    },
    {
        "name": "commands",
        "theme": "day",
        "video": True,
        "keys": pointer("Click", 1, CAPTURE_WINDOW["rows"] - 1) + ("Wait 900",),
    },
    {
        "name": "commands",
        "theme": "night",
        "video": True,
        "keys": pointer("Click", 1, CAPTURE_WINDOW["rows"] - 1) + ("Wait 900",),
    },
    {
        "name": "triage",
        "theme": "day",
        "video": True,
        "keys": pointer("Click", 8, 28) + ("Wait 900",),
    },
    {
        "name": "triage",
        "theme": "night",
        "video": True,
        "keys": pointer("Click", 8, 28) + ("Wait 900",),
    },
)

PUBLIC_DIR = REPO_ROOT / "website" / "public"
PUBLIC_NAMES = {
    "workspace": "session",
    "groups": "groups",
    "agents": "agents",
    "follow-up": "follow-up",
    "collapsed-status": "collapsed",
    "commands": "commands",
    "triage": "triage",
}




OUT_DIR = REPO_ROOT / "demo" / "cap" / "out"
CLICLICK_CANDIDATES = (
    "/opt/homebrew/bin/cliclick",
    "/usr/local/bin/cliclick",
)
CAP_CANDIDATES = (
    str(Path.home() / ".local/bin/cap"),
    "/Applications/Cap.app/Contents/MacOS/cap-cli",
)


def shot_stem(shot: dict[str, Any]) -> str:
    return f"{shot['name']}-{shot['theme']}"


def public_stem(shot: dict[str, Any]) -> str | None:
    name = PUBLIC_NAMES.get(shot["name"])
    if name is None:
        return None
    if shot["theme"] == "night":
        return f"{name}-night"
    return name


def media_height_px(path: Path) -> int:
    if path.suffix == ".png":
        from PIL import Image

        with Image.open(path) as image:
            return image.size[1]

    completed = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=height",
            "-of",
            "csv=p=0",
            str(path),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    return int((completed.stdout or "0").strip() or 0)


def titlebar_crop_px(path: Path, png_source: Path) -> int:
    bar = int(CAPTURE_WINDOW.get("pad_top") or 0)
    if bar <= 0 or not path.is_file() or not png_source.is_file():
        return 0
    from PIL import Image

    with Image.open(png_source) as image:
        png_height = image.size[1]
    scale = 2 if png_height >= 1600 else 1
    logical_height = png_height / scale
    file_height = media_height_px(path)
    if logical_height <= 0 or file_height <= 0:
        return 0
    return max(0, round(file_height * bar / logical_height))



def crop_published_titlebar(path: Path, bar: int) -> None:
    if bar <= 0 or not path.is_file():
        return
    if path.suffix == ".png":
        from PIL import Image

        image = Image.open(path)
        cropped = image.crop((0, bar, image.size[0], image.size[1]))
        cropped.save(path)
        return
    if path.suffix == ".mp4":
        tmp = path.with_suffix(".crop.mp4")
        completed = subprocess.run(
            [
                "ffmpeg",
                "-y",
                "-i",
                str(path),
                "-vf",
                f"crop=iw:ih-{bar}:0:{bar}",
                str(tmp),
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode == 0 and tmp.is_file():
            tmp.replace(path)


def publish_shot_assets(shot: dict[str, Any]) -> None:
    public = public_stem(shot)
    if public is None:
        return
    stem = shot_stem(shot)
    PUBLIC_DIR.mkdir(parents=True, exist_ok=True)
    png_source = OUT_DIR / f"{stem}.png"
    for suffix in (".png", ".mp4"):
        source = OUT_DIR / f"{stem}{suffix}"
        if source.is_file():
            dest = PUBLIC_DIR / f"{public}{suffix}"
            shutil.copy2(source, dest)
            crop_published_titlebar(dest, titlebar_crop_px(dest, png_source))




def resolve_shots(name: str, theme: str) -> list[dict[str, Any]]:
    if name == "all":
        named = list(SHOTS)
    else:
        named = [shot for shot in SHOTS if shot["name"] == name]
    if not named:
        raise ValueError(f"unknown shot: {name}")
    if theme == "all":
        return named
    if theme not in THEMES:
        raise ValueError(f"unknown theme: {theme}")
    matched = [shot for shot in named if shot["theme"] == theme]
    if matched:
        return matched
    return [{**named[0], "theme": theme}]


def key_to_cliclick(key: str, window: dict[str, Any]) -> list[str]:
    if key.startswith("Ctrl+") and len(key) == 6:
        letter = key[-1].lower()
        return ["kd:ctrl", f"t:{letter}", "ku:ctrl"]
    if key == "Shift+F10":
        return ["kd:shift", "kp:f10", "ku:shift"]
    if key.startswith("Type `") and key.endswith("`"):
        return [f"t:{key[6:-1]}"]
    if key == "Space":
        return ["kp:space"]
    if key == "Enter":
        return ["kp:return"]
    if key == "Down":
        return ["kp:arrow-down"]
    if key.startswith("Wait "):
        return [f"w:{int(key.split()[1])}"]
    kind, _, coords = key.partition(" ")
    if kind.endswith("Cell") and coords:
        action = kind[: -len("Cell")]
        col_text, _, row_text = coords.partition(",")
        pad_top = int(CAPTURE_WINDOW.get("pad_top") or 0)
        cell_w = int(window["width"]) / CAPTURE_WINDOW["columns"]
        cell_h = (int(window["height"]) - pad_top) / CAPTURE_WINDOW["rows"]
        x = int(window["x"] + float(col_text) * cell_w + cell_w / 2)
        y = int(window["y"] + pad_top + float(row_text) * cell_h + cell_h / 2)
        prefix = {"Click": "c", "RightClick": "rc", "Move": "m"}[action]
        return [f"{prefix}:{x},{y}"]

    if kind in {"Click", "RightClick", "Move"} and coords:
        x_text, _, y_text = coords.partition(",")
        x = int(window["x"]) + int(x_text)
        y = int(window["y"]) + int(y_text)
        prefix = {"Click": "c", "RightClick": "rc", "Move": "m"}[kind]
        return [f"{prefix}:{x},{y}"]
    raise ValueError(f"unknown workflow step: {key}")





def cliclick_commands(keys: tuple[str, ...], window: dict[str, Any]) -> list[str]:
    if not keys:
        return []
    commands: list[str] = []
    for key in keys:
        commands.extend(key_to_cliclick(key, window))
        commands.append("w:500")
    return commands


def resolve_cap_window_id(windows: list[dict[str, Any]], title: str) -> str:
    matches = [window for window in windows if window.get("name") == title]
    if not matches:
        names = [window.get("name") for window in windows]
        raise RuntimeError(f"Cap did not see window {title!r}; visible={names}")
    if len(matches) > 1:
        raise RuntimeError(f"multiple Cap windows named {title!r}")
    return str(matches[0]["id"])



def cap_window_geometry(windows: list[dict[str, Any]], title: str) -> dict[str, Any]:
    match = next(window for window in windows if window.get("name") == title)
    bounds = match.get("bounds") or {}
    return {
        "x": int(bounds.get("x") or match.get("x") or 0),
        "y": int(bounds.get("y") or match.get("y") or 0),
        "width": int(bounds.get("width") or match.get("width") or 0),
        "height": int(bounds.get("height") or match.get("height") or 0),
    }



def parse_ndjson(text: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for line in text.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        rows.append(json.loads(line))
    return rows


def cap_cmd(cap: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(cap), "--json", *args],
        check=False,
        capture_output=True,
        text=True,
    )


def session_exists(home: Path) -> bool:
    try:
        session_path(home)
        return True
    except FileNotFoundError:
        return False


def ensure_demo(bin_path: Path, home: Path, reset: bool) -> None:
    if reset or not session_exists(home):
        seed(bin_path, home, reset=reset)
        return
    install_fixture(home)


def which_tool(name: str, candidates: tuple[str, ...] = ()) -> Path | None:
    found = shutil.which(name)
    if found:
        return Path(found)
    for candidate in candidates:
        path = Path(candidate).expanduser()
        if path.is_file() and os.access(path, os.X_OK):
            return path
    return None


def check_deps(bin_path: Path) -> list[str]:
    missing: list[str] = []
    try:
        import PIL.Image
    except ImportError:
        missing.append("Pillow (python3 -m pip install Pillow)")
    for tool in ("ffprobe", "ffmpeg"):
        if which_tool(tool) is None:
            missing.append(f"{tool} (brew install ffmpeg)")
    if which_tool("cap", CAP_CANDIDATES) is None:
        missing.append("cap CLI (install Cap.app, then `cap desktop install` or put cap on PATH)")
    if which_tool("cliclick", CLICLICK_CANDIDATES) is None:
        missing.append("cliclick (brew install cliclick)")
    if not GHOSTTY_APP.exists():
        missing.append(f"Ghostty.app at {GHOSTTY_APP}")
    if not bin_path.is_file():
        missing.append(f"gardn binary at {bin_path} (build it or set GARDN_BIN)")
    elif not os.access(bin_path, os.X_OK):
        missing.append(f"gardn binary is not executable: {bin_path}")
    if not Path("/usr/bin/python3").is_file():
        missing.append("/usr/bin/python3 (needed for window listing)")
    cap = which_tool("cap", CAP_CANDIDATES)
    if cap is not None:
        doctor = cap_cmd(cap, "doctor")
        if doctor.returncode != 0:
            missing.append(f"cap doctor failed: {doctor.stderr.strip() or doctor.stdout.strip()}")
        else:
            payload = json.loads(doctor.stdout or "{}")
            if not payload.get("captureReady"):
                missing.append("cap doctor reports captureReady=false")
            permissions = payload.get("permissions") or {}
            if permissions.get("screenRecording") != "granted":
                missing.append("Cap screen recording permission is not granted")
    try:
        read_os_dark_mode()
    except RuntimeError as exc:
        missing.append(str(exc))
    return missing



def list_cap_windows(cap: Path) -> list[dict[str, Any]]:
    completed = cap_cmd(cap, "record", "windows")
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "cap record windows failed")
    payload = json.loads(completed.stdout)
    if not isinstance(payload, list):
        raise RuntimeError(f"unexpected cap record windows payload: {payload!r}")
    return payload


def run_cliclick(cliclick: Path, commands: list[str]) -> None:
    completed = subprocess.run(
        [str(cliclick), *commands],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "cliclick failed")



def start_recording(cap: Path, window_id: str, project: Path) -> str:
    project.parent.mkdir(parents=True, exist_ok=True)
    completed = cap_cmd(
        cap,
        "record",
        "start",
        "--window",
        window_id,
        "--detach",
        "--path",
        str(project),
    )
    events = parse_ndjson(completed.stdout)
    started = next((event for event in events if event.get("type") == "started"), None)
    if completed.returncode != 0 or started is None:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "cap record start failed")
    recording_id = started.get("recordingId") or started.get("id")
    if not recording_id:
        raise RuntimeError(f"cap record start did not return recordingId: {started}")
    return str(recording_id)


def stop_recording(cap: Path, recording_id: str) -> None:
    completed = cap_cmd(cap, "record", "stop", "--id", recording_id)
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "cap record stop failed")


def export_recording(cap: Path, project: Path, output: Path) -> None:
    completed = cap_cmd(
        cap,
        "export",
        str(project),
        str(output),
        "--resolution",
        f"{CAPTURE_WINDOW['width']}x{CAPTURE_WINDOW['height']}",
    )
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "cap export failed")


def screenshot_window(cap: Path, window_id: str, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    completed = cap_cmd(cap, "screenshot", "--window", window_id, "--path", str(path))
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "cap screenshot failed")


def theme_wants_dark_os(theme: str) -> bool:
    return theme == "night"


def read_os_dark_mode() -> bool:
    completed = subprocess.run(
        [
            "osascript",
            "-e",
            'tell application "System Events" to tell appearance preferences to get dark mode',
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            completed.stderr.strip()
            or "cannot read macOS appearance (grant Automation for System Events)"
        )
    return completed.stdout.strip().lower() == "true"


def set_os_dark_mode(dark: bool) -> None:
    value = "true" if dark else "false"
    completed = subprocess.run(
        [
            "osascript",
            "-e",
            f'tell application "System Events" to tell appearance preferences to set dark mode to {value}',
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            completed.stderr.strip()
            or "cannot set macOS appearance (grant Automation for System Events)"
        )


def prepare_shot(
    bin_path: Path,
    home: Path,
    theme: str,
    *,
    sidebar_collapsed: bool = False,
) -> dict[str, Any]:
    set_os_dark_mode(theme_wants_dark_os(theme))
    apply_theme(home, theme)
    close_capture_window()
    time.sleep(0.4)
    stop_server(bin_path, home)
    start_server(bin_path, home)
    prepare_demo_runtime(bin_path, home, sidebar_collapsed=sidebar_collapsed)
    return open_capture_window(bin_path, home, theme)


def capture_shot(
    shot: dict[str, Any],
    bin_path: Path,
    home: Path,
    cap: Path,
    cliclick: Path,
    *,
    video: bool,
) -> None:
    opened = prepare_shot(
        bin_path,
        home,
        shot["theme"],
        sidebar_collapsed=bool(shot.get("sidebar_collapsed")),
    )
    hidden = list(opened.get("hidden_ghostty") or ())
    try:
        focus = shot.get("focus")
        if focus:
            focus_named_workspace_tab(bin_path, home, focus[0], focus[1])

        report_demo_states(bin_path, home)
        time.sleep(0.3)
        cap_windows = list_cap_windows(cap)
        window_id = resolve_cap_window_id(cap_windows, CAPTURE_WINDOW_TITLE)
        matches = [window for window in list_ghostty_windows() if is_capture_window(window)]
        if not matches:
            raise RuntimeError("Quartz did not see the Gardn Demo Capture window")
        hit = {
            "x": int(matches[0]["x"]),
            "y": int(matches[0]["y"]),
            "width": int(matches[0]["width"]),
            "height": int(matches[0]["height"]),
        }
        commands = cliclick_commands(tuple(shot.get("keys") or ()), hit)
        focus_capture_process(int(matches[0]["pid"]))
        time.sleep(0.2)

        stem = shot_stem(shot)
        OUT_DIR.mkdir(parents=True, exist_ok=True)
        png = OUT_DIR / f"{stem}.png"
        recording_id = None
        project = OUT_DIR / f"{stem}.cap"
        mp4 = OUT_DIR / f"{stem}.mp4"
        if video and shot.get("video"):
            recording_id = start_recording(cap, window_id, project)
            focus_capture_process(int(matches[0]["pid"]))
            time.sleep(0.2)
        try:
            if commands:
                run_cliclick(cliclick, commands)
            if shot["name"] == "workspace":
                focus_showcase(bin_path, home)
                report_demo_states(bin_path, home)
                time.sleep(0.3)
            else:
                time.sleep(0.4)
            cap_windows = list_cap_windows(cap)
            window_id = resolve_cap_window_id(cap_windows, CAPTURE_WINDOW_TITLE)
            screenshot_window(cap, window_id, png)
            if recording_id is not None:
                time.sleep(1.2)
        finally:
            if recording_id is not None:
                stop_recording(cap, recording_id)
                export_recording(cap, project, mp4)
    finally:
        restore_other_ghostty(hidden)
        close_capture_window()



def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("shot", help="shot name from SHOTS, all, or deps")
    parser.add_argument(
        "--theme",
        default="day",
        choices=("day", "night", "all"),
        help="day, night, or all matching SHOTS rows",
    )
    parser.add_argument("--reset", action="store_true", help="wipe the isolated session first")
    parser.add_argument("--no-video", action="store_true", help="write stills only")
    parser.add_argument("--home", type=Path, default=default_home())
    parser.add_argument("--bin", type=Path, default=default_bin())
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    home = args.home.expanduser()
    bin_path = args.bin.expanduser()
    missing = check_deps(bin_path)
    if args.shot == "deps":
        if missing:
            print("missing:", file=sys.stderr)
            for item in missing:
                print(f"  {item}", file=sys.stderr)
            return 2
        print("cap\t" + str(which_tool("cap", CAP_CANDIDATES)))
        print("cliclick\t" + str(which_tool("cliclick", CLICLICK_CANDIDATES)))
        print("ghostty\t" + str(GHOSTTY_APP))
        print("gardn\t" + str(bin_path))
        return 0
    if missing:
        print("missing dependencies:", file=sys.stderr)
        for item in missing:
            print(f"  {item}", file=sys.stderr)
        return 2
    try:
        shots = resolve_shots(args.shot, args.theme)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    cap = which_tool("cap", CAP_CANDIDATES)
    cliclick = which_tool("cliclick", CLICLICK_CANDIDATES)
    if cap is None or cliclick is None:
        return 2
    ensure_demo(bin_path, home, reset=args.reset)
    start_server(bin_path, home)
    previous_dark = read_os_dark_mode()
    try:
        for shot in shots:
            capture_shot(
                shot,
                bin_path,
                home,
                cap,
                cliclick,
                video=not args.no_video,
            )
            publish_shot_assets(shot)

    finally:
        set_os_dark_mode(previous_dark)
    return 0



if __name__ == "__main__":
    raise SystemExit(main())
