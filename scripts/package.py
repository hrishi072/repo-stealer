"""Package the CLI (not a desktop UI) using native OS tools."""
import hashlib
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
OUT = ROOT / "dist"
OUT.mkdir(exist_ok=True)
WINDOWS = os.name == "nt"
BINARY = ROOT / "target/release" / ("repo-stealer.exe" if WINDOWS else "repo-stealer")


def call(*args, **kwargs):
    subprocess.run(list(map(str, args)), check=True, **kwargs)


def copy_contents(path):
    path.mkdir(parents=True, exist_ok=True)
    shutil.copy2(BINARY, path / BINARY.name)
    shutil.copy2(ROOT / "README.md", path)
    shutil.copytree(ROOT / "docs", path / "docs")
    (path / "BUILD-INFO.txt").write_text(
        f"Repo Stealer CLI {VERSION}\nCommit: {os.environ.get('GITHUB_SHA', 'local')}\n"
        "Experimental encrypted storage/backup CLI. No Git transfers or desktop UI yet.\n"
        "Source license: LicenseRef-Proprietary. No open-source grant.\n", encoding="utf-8")


with tempfile.TemporaryDirectory() as tmp:
    stage = pathlib.Path(tmp)
    portable = stage / f"repo-stealer-{VERSION}"
    copy_contents(portable)
    if WINDOWS:
        shutil.copy2(BINARY, OUT / "repo-stealer-windows-x86_64.exe")
        with zipfile.ZipFile(OUT / f"repo-stealer-{VERSION}-windows-x86_64.zip", "w", zipfile.ZIP_DEFLATED) as archive:
            for path in portable.rglob("*"):
                if path.is_file():
                    archive.write(path, path.relative_to(stage))
    else:
        with tarfile.open(OUT / f"repo-stealer-{VERSION}-linux-x86_64.tar.gz", "w:gz") as archive:
            archive.add(portable, arcname=portable.name)
        deb = stage / "deb"
        (deb / "DEBIAN").mkdir(parents=True)
        (deb / "usr/bin").mkdir(parents=True)
        shutil.copy2(BINARY, deb / "usr/bin/repo-stealer")
        (deb / "DEBIAN/control").write_text(
            f"Package: repo-stealer\nVersion: {VERSION.replace('-', '~')}\nArchitecture: amd64\n"
            "Maintainer: hrishi072 <parundekarrushi@gmail.com>\n"
            "Depends: libc6 (>= 2.35), libgcc-s1\nSection: utils\nPriority: optional\n"
            "Description: Experimental encrypted repository storage and backup CLI\n")
        call("dpkg-deb", "--root-owner-group", "--build", deb, OUT / f"repo-stealer-{VERSION}-amd64.deb")
        rpm = stage / "rpm"
        for directory in ("BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS", "BUILDROOT"):
            (rpm / directory).mkdir(parents=True)
        shutil.copy2(BINARY, rpm / "SOURCES/repo-stealer")
        base, _, suffix = VERSION.partition("-")
        release = "0." + suffix if suffix else "1"
        spec = rpm / "SPECS/repo-stealer.spec"
        spec.write_text(f"""Name: repo-stealer
Version: {base}
Release: {release}
Summary: Experimental encrypted storage and backup CLI
License: LicenseRef-Proprietary
BuildArch: x86_64
Source0: repo-stealer
%global debug_package %{{nil}}
%description
Encrypted local objects and same-key backups. No Git transport or desktop UI yet.
%install
install -D -m 0755 %{{SOURCE0}} %{{buildroot}}/usr/bin/repo-stealer
%files
/usr/bin/repo-stealer
""")
        call("rpmbuild", "--define", f"_topdir {rpm}", "-bb", spec)
        for path in (rpm / "RPMS").rglob("*.rpm"):
            shutil.copy2(path, OUT / path.name)
        appdir = stage / "RepoStealer.AppDir"
        (appdir / "usr/bin").mkdir(parents=True)
        shutil.copy2(BINARY, appdir / "usr/bin/repo-stealer")
        launcher = appdir / "AppRun"
        launcher.write_text('#!/bin/sh\nHERE="$(dirname "$(readlink -f "$0")")"\nexec "$HERE/usr/bin/repo-stealer" "$@"\n')
        launcher.chmod(0o755)
        (appdir / "repo-stealer.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Repo Stealer CLI\nExec=repo-stealer\n"
            "Icon=repo-stealer\nTerminal=true\nCategories=Utility;\n")
        (appdir / "repo-stealer.svg").write_text(
            '<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">'
            '<rect width="128" height="128" rx="24" fill="#172638"/>'
            '<path d="M40 32v64m0-32h48V32" fill="none" stroke="#59dfc2" stroke-width="10"/>'
            '<g fill="#59dfc2"><circle cx="40" cy="32" r="12"/><circle cx="40" cy="96" r="12"/>'
            '<circle cx="88" cy="32" r="12"/></g></svg>')
        tool = stage / "appimagetool.AppImage"
        urllib.request.urlretrieve(
            "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage", tool)
        assert hashlib.sha256(tool.read_bytes()).hexdigest() == "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0", "appimagetool checksum mismatch"
        tool.chmod(0o755)
        image = OUT / f"repo-stealer-{VERSION}-x86_64.AppImage"
        call(tool, "--appimage-extract-and-run", appdir, image,
             env={**os.environ, "ARCH": "x86_64", "APPIMAGE_EXTRACT_AND_RUN": "1"})
        call(image, "--appimage-extract-and-run", "--help")

print("Packaged", VERSION, "for", "Windows" if WINDOWS else "Linux")
