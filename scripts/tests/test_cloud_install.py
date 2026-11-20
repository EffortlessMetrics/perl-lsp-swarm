"""Exercise cloud-install with local archives and inert tool shims."""
import hashlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class CloudInstallTest(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        self.repo = root / "repo"
        self.shared = root / "shared"
        self.bin = root / "bin"
        self.bin.mkdir()
        self.files = root / "files"
        self.files.mkdir()
        self.log = root / "log"
        self.log.touch()
        for path in ("scripts/cloud-install", "scripts/cloud-startup", "scripts/lib/cargo-toolchain-guard.sh"):
            dest = self.repo / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / path, dest)
        (self.repo / "scripts/check_release_tag_provenance.py").touch()
        self.repo.joinpath("Cargo.toml").write_text('[workspace.package]\nrust-version = "1.95"\n')
        self.repo.joinpath("rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.96.1"\nprofile = "minimal"\ncomponents = ["rustfmt","clippy"]\n')
        action = self.repo / ".github/actions/setup-vscode-toolchain/action.yml"
        action.parent.mkdir(parents=True)
        action.write_text("        node-version: '26.5.0'\n")
        extension = self.repo / "vscode-extension"
        extension.mkdir()
        (extension / "package.json").write_text('{"packageManager":"npm@11.18.0"}')
        self.env = dict(PATH=str(self.bin), PERL_LSP_CLOUD_SHARED=str(self.shared),
                        FILES=str(self.files), LOG=str(self.log), RUSTUP_TOOLCHAIN="stable",
                        HOME=str(root))
        for tool in ("bash", "sh", "git", "python3", "perl", "sed", "awk", "dirname", "mktemp",
                     "mkdir", "rm", "tar", "xz", "mv", "ln", "sha256sum", "uname", "cp", "chmod", "cat"):
            (self.bin / tool).symlink_to(shutil.which(tool))
        self.shell(self.bin / "curl", '''
            while [ "$1" != "-o" ]; do url=$1; shift; done
            echo "$url" >> "$LOG"
            case "$url" in
              https://sh.rustup.rs) file=rust-init ;;
              https://astral.sh/uv/install.sh) file=uv-init ;;
              https://just.systems/install.sh) file=just-init ;;
              https://nodejs.org/dist/latest/SHASUMS256.txt) file=current.sum ;;
              https://nodejs.org/dist/v26.5.0/SHASUMS256.txt) file=project.sum ;;
              https://nodejs.org/dist/*/*.tar.xz) file=${url##*/} ;;
              *) exit 71 ;;
            esac
            cp "$FILES/$file" "$2"
        ''')
        self.shell(self.files / "rust-init", '''
            [ "$*" = "-y --no-modify-path --default-toolchain none" ]
            mkdir -p "$CARGO_HOME/bin"
            cp "$FILES/rustup" "$CARGO_HOME/bin/"
            for tool in cargo rustc; do cp "$FILES/version" "$CARGO_HOME/bin/$tool"; done
        ''')
        self.shell(self.files / "version", "echo 'fixture 1.96.1'")
        self.shell(self.files / "rustup", '''
            echo "rustup $*" >> "$LOG"
            case "$*" in
              --help) echo "rustup ${MOCK_MANAGER:-1.28.2} (fixture)" ;;
              'which --toolchain 1.96.1 rustc') [ "${RUSTUP_AUTO_INSTALL-}" = 0 ] && [ -z "${MISSING_PIN-}" ] ;;
              'which --toolchain stable rustc') [ "${RUSTUP_AUTO_INSTALL-}" = 0 ] ;;
              'show active-toolchain') echo "startup selector ${RUSTUP_TOOLCHAIN-unset}" >> "$LOG"
                  echo "${RUSTUP_TOOLCHAIN:-1.96.1}-fixture" ;;
              'run 1.96.1 rustc --version') echo "rustc ${MOCK_PIN:-1.96.1} (fixture)" ;;
              'run stable rustc --version') echo "rustc ${MOCK_STABLE:-1.99.0} (fixture)" ;;
              'run 1.96.1 cargo --version'|'run 1.96.1 cargo fetch --locked'|\
              'run 1.96.1 cargo metadata --frozen --format-version 1'|\
              'toolchain install stable --profile minimal --no-self-update'|\
              'toolchain install 1.96.1 --profile minimal --component rustfmt,clippy --no-self-update') ;;
              *) exit 72 ;;
            esac
        ''')
        self.shell(self.files / "uv-init", 'cp "$FILES/version" "$UV_UNMANAGED_INSTALL/uv"')
        self.shell(self.files / "just-init", 'cp "$FILES/version" "$2/just"')
        self.shell(self.files / "npm", '''
            echo "npm $(node --version) $* selector=${RUSTUP_TOOLCHAIN-unset} npm=$0 cwd=$PWD" >> "$LOG"
            case "$*" in
              --version) echo '11.18.0' ;;
              'run doctor'|ci) [ "$(node --version)" = v26.5.0 ] ;;
              *) exit 73 ;;
            esac
        ''')
        arch = {"x86_64": "x64", "aarch64": "arm64"}[os.uname().machine]
        for version, label in (("27.1.0", "current"), ("26.5.0", "project")):
            stem = f"node-v{version}-linux-{arch}"
            archive = self.files / f"{stem}.tar.xz"
            with tarfile.open(archive, "w:xz") as tar:
                for tool, body in (
                    ("node", f'case "$*" in --version) echo v{version};; scripts/toolchain-doctor.js) npm --version;; *) exit 74;; esac'),
                    ("npm", 'test "$1 $2 $3" = "install --global --prefix" && test "$5" = npm@11.18.0 && mkdir -p "$4/bin" && cp "$FILES/npm" "$4/bin/npm"'),
                ):
                    data = f"#!/bin/sh\n{body}\n".encode()
                    info = tarfile.TarInfo(f"{stem}/bin/{tool}")
                    info.size, info.mode = len(data), 0o755
                    tar.addfile(info, io.BytesIO(data))
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            (self.files / f"{label}.sum").write_text(f"{digest}  {archive.name}\n")

    @staticmethod
    def shell(path, body):
        path.write_text("#!/bin/sh\nset -eu\n" + body)
        path.chmod(0o755)

    def invoke(self, *args, ok=True, error=None, **env):
        self.log.write_text("")
        effective_env = {key: value for key, value in {**self.env, **env}.items() if value is not None}
        result = subprocess.run(["/bin/bash", "scripts/cloud-install", *args], cwd=self.repo,
                                env=effective_env, capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode == 0, ok, result.stderr)
        if error is not None:
            self.assertIn(error, result.stderr)
        return self.log.read_text()

    def test_cold_repeat_and_offline(self):
        # Current main's historical Node path must not shadow the project runtime.
        historical = self.shared / "node-v26.6.0-linux-x64/bin"
        historical.mkdir(parents=True)
        self.shell(historical / "node", "exit 76")
        cold = self.invoke()
        self.assertIn("https://sh.rustup.rs", cold)
        self.assertIn("https://nodejs.org/dist/v27.1.0/node-", cold)
        self.assertIn("npm v26.5.0 ci", cold)
        repeat = self.invoke()
        self.assertNotIn("https://sh.rustup.rs", repeat)
        self.assertNotIn("https://just.systems", repeat)
        self.assertNotIn(".tar.xz", repeat)
        self.assertIn("npm v26.5.0 ci", repeat)
        check = self.invoke("--check")
        self.assertIn("cargo metadata --frozen", check)
        for effect in ("https://", " fetch ", " install ", " ci"):
            self.assertNotIn(effect, check)
        self.invoke("--check", ok=False, MOCK_PIN="1.99.0")
        for override in ({"MISSING_PIN": "1"}, {"MOCK_MANAGER": "1.27.0"}):
            with self.subTest(override=override):
                refused = self.invoke("--check", ok=False, **override)
                self.assertNotIn("rustup run ", refused)
                self.assertNotIn("https://", refused)

    def test_installer_preserves_selector_through_project_npm_ci(self):
        for selector in ("stable", None):
            with self.subTest(selector=selector):
                log = self.invoke(RUSTUP_TOOLCHAIN=selector)
                selected = selector or "unset"
                self.assertIn(f"startup selector {selected}\n", log)
                npm = self.shared / "codex-tools/extension-npm/bin/npm"
                extension = self.repo / "vscode-extension"
                for args in ("run doctor", "ci"):
                    self.assertIn(f"npm v26.5.0 {args} selector={selected} npm={npm} cwd={extension}", log)
                self.assertIn("rustup run 1.96.1 cargo fetch --locked", log)
                self.assertIn("rustup run 1.96.1 cargo metadata --frozen", log)

    def test_below_msrv_stable_stops_install_and_offline_check(self):
        self.invoke()
        for args in ((), ("--check",)):
            with self.subTest(args=args):
                refused = self.invoke(*args, ok=False, MOCK_STABLE="1.94.0", error="Expected stable rustc >= 1.95")
                self.assertIn("rustup run stable rustc --version", refused)
                self.assertNotIn("cargo metadata", refused)
                self.assertNotIn("npm v26.5.0 ci", refused)
        self.invoke("--check", MOCK_STABLE="1.95.0")

    def test_task_retains_just_from_install_only_path(self):
        transient = self.files / "install-only"
        transient.mkdir()
        self.shell(transient / "just", 'echo "prepared just $*" >> "$LOG"')
        self.invoke(PATH=f"{transient}:{self.bin}")
        shutil.rmtree(transient)
        self.assertIn("prepared just --version", self.invoke("--check"))

    def test_install_and_check_refuse_relative_shared_path_before_provisioning(self):
        for args in ((), ("--check",)):
            with self.subTest(args=args):
                self.assertEqual(self.invoke(*args, ok=False, PERL_LSP_CLOUD_SHARED="relative"), "")
                self.assertFalse((self.repo / "relative").exists())

    def test_fresh_shell_reuses_preinstalled_manager(self):
        image_bin = self.files / "image-bin"
        image_bin.mkdir()
        shutil.copy(self.files / "rustup", image_bin / "rustup")
        for tool in ("cargo", "rustc"):
            shutil.copy(self.files / "version", image_bin / tool)
        log = self.invoke(PATH=f"{image_bin}:{self.bin}")
        self.assertNotIn("https://sh.rustup.rs", log)
        self.assertTrue((self.shared / "codex-tools/rust-bin").samefile(image_bin))
        self.assertIn("cargo metadata --frozen", self.invoke("--check"))

    def test_shadow_in_cargo_bin_refuses_before_extension_install(self):
        self.invoke()
        for tool in ("node", "npm"):
            with self.subTest(tool=tool):
                shadow = self.shared / "cargo/bin" / tool
                self.shell(shadow, 'exit 76')
                try:
                    log = self.invoke(ok=False, error=f"shadows the prepared extension {tool}")
                    self.assertNotIn("npm v26.5.0 ci", log)
                finally:
                    shadow.unlink()

    def test_corrupt_archive_refused(self):
        archive = next(self.files.glob("*27.1.0*.xz"))
        archive.write_bytes(b"corrupt")
        self.assertNotIn("npm v26.5.0 ci", self.invoke(ok=False))
        self.assertFalse((self.shared / "codex-tools/node-current-bin").exists())
