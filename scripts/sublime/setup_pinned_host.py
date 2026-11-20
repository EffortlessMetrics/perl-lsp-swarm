"""Install the reviewed Sublime real-host inputs on a fresh CI runner."""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile


ARTIFACTS = {
    "Linux": (
        "sublime_text_build_4200_x64.tar.xz",
        "36f69c551ad18ee46002be4d9c523fe545d93b67fea67beea731e724044b469f",
    ),
    "macOS": (
        "sublime_text_build_4200_mac.zip",
        "4835eb2a5d3f2b223ce93a27149f360ef158af9f8dd708b6f501d708c081d319",
    ),
    "Windows": (
        "sublime_text_build_4200_x64.zip",
        "d20456bbefcd626c7c89a4a2e95c326a0c570df2fd7626fc35091e43ae5bff9f",
    ),
}


def install_package(destination, repository, commit):
    with tempfile.TemporaryDirectory(dir=os.environ["RUNNER_TEMP"]) as checkout:
        subprocess.run(["git", "init", "--quiet", checkout], check=True)
        subprocess.run(
            ["git", "-C", checkout, "fetch", "--quiet", "--depth", "1",
             f"https://github.com/{repository}.git", commit],
            check=True,
        )
        subprocess.run(
            ["git", "-C", checkout, "checkout", "--quiet", "--detach", "FETCH_HEAD"],
            check=True,
        )
        actual = subprocess.check_output(
            ["git", "-C", checkout, "rev-parse", "HEAD"], text=True
        ).strip()
        if actual != commit:
            raise SystemExit(f"{destination.name}: expected {commit}, got {actual}")
        shutil.copytree(checkout, destination, ignore=shutil.ignore_patterns(".git"))
    print(f"{destination.name}: {actual}", flush=True)


def main():
    runner_os = os.environ["RUNNER_OS"]
    artifact, expected = ARTIFACTS[runner_os]
    with tempfile.TemporaryDirectory(dir=os.environ["RUNNER_TEMP"]) as staging:
        archive = Path(staging) / artifact
        subprocess.run(
            ["curl", "-fsSL", "--retry", "3",
             f"https://download.sublimetext.com/{artifact}", "-o", archive],
            check=True,
        )
        with archive.open("rb") as handle:
            actual = hashlib.sha256()
            for block in iter(lambda: handle.read(1024 * 1024), b""):
                actual.update(block)
        digest = actual.hexdigest()
        if digest != expected:
            raise SystemExit(
                f"SHA-256 mismatch for {artifact}: expected {expected}, got {digest}"
            )

        install = Path(tempfile.mkdtemp(prefix="sublime-4200-", dir=os.environ["RUNNER_TEMP"]))
        if runner_os == "Windows":
            with zipfile.ZipFile(archive) as bundle:
                bundle.extractall(install)
            launcher = install / "sublime_text.exe"
            plugin_host = install / "plugin_host-3.8.exe"
            packages = install / "Data" / "Packages"
        elif runner_os == "macOS":
            subprocess.run(["unzip", "-q", archive, "-d", install], check=True)
            app = install / "Sublime Text.app"
            launcher = app / "Contents/SharedSupport/bin/subl"
            plugin_host = app / "Contents/MacOS/plugin_host-3.8"
            packages = Path.home() / "Library/Application Support/Sublime Text/Packages"
            subprocess.run(
                [
                    "/System/Library/Frameworks/CoreServices.framework/Frameworks/"
                    "LaunchServices.framework/Support/lsregister",
                    "-f",
                    app,
                ],
                check=True,
            )
        else:
            subprocess.run(["tar", "xf", archive, "-C", install], check=True)
            launcher = install / "sublime_text/sublime_text"
            plugin_host = launcher.with_name("plugin_host-3.8")
            config_home = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
            packages = config_home / "sublime-text/Packages"

    if not launcher.is_file() or not plugin_host.is_file():
        raise SystemExit(f"{artifact} lacks the expected launcher or Python 3.8 plugin host")
    print(f"Sublime Text build 4200: {artifact} SHA-256 {expected}", flush=True)

    user = packages / "User"
    user.mkdir(parents=True, exist_ok=True)
    preferences = user / "Preferences.sublime-settings"
    if not preferences.exists():
        preferences.write_text(
            '{"close_windows_when_empty": false, "update_check": false}\n', encoding="utf-8"
        )
    install_package(
        packages / "UnitTesting", "SublimeText/UnitTesting",
        "b8abedefd5fbc0f64a8c6082506ebd1e24203164",
    )
    install_package(packages / "LSP", "sublimelsp/LSP", os.environ["PERLLSP_LSP_REF"])
    install_package(
        packages / "sublime_aio", "packagecontrol/sublime_aio",
        "b3d1d559ec223f276c9693e17476768257951266",
    )
    shutil.copytree("clients/sublime/LSP-perllsp", packages / "LSP-perllsp")

    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as env_file:
        env_file.write(f"SUBLIME_TEXT_PACKAGES={packages.as_posix()}\n")
        env_file.write(f"PERLLSP_SUBLIME_LAUNCHER={launcher.as_posix()}\n")


if __name__ == "__main__":
    main()
