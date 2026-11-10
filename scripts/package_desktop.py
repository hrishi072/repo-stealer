"""Name desktop assets distinctly from the CLI release assets."""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
desktop = root / "apps/desktop"
version = json.loads((desktop / "package.json").read_text())["version"]
bundle = desktop / "src-tauri/target/release/bundle"
if sys.argv[1:] == ["--clean-bundle"]:
    assert not bundle.is_symlink(), "refuse a symlinked generated bundle directory"
    if bundle.exists():
        shutil.rmtree(bundle)
    print("Removed generated desktop installer outputs before bundling")
    raise SystemExit(0)
assert len(sys.argv)==1, "usage: package_desktop.py [--clean-bundle]"
out = root / "dist-desktop"
out.mkdir(exist_ok=True)
platform = "windows" if os.name == "nt" else "linux"
count = 0
if os.name != "nt":
    # Tauri's RPM bundler currently passes the SemVer prerelease hyphen to
    # RPM, which rejects it. Keep the app version and map only RPM metadata.
    with tempfile.TemporaryDirectory(prefix="rs-desktop-rpm-") as temporary:
        rpm = pathlib.Path(temporary)
        for directory in ("BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS", "BUILDROOT"):
            (rpm / directory).mkdir()
        shutil.copy2(desktop / "src-tauri/target/release/repo-stealer-desktop", rpm / "SOURCES/repo-stealer-desktop")
        shutil.copy2(desktop / "src-tauri/icons/128x128.png", rpm / "SOURCES/repo-stealer.png")
        entry = rpm / "SOURCES/com.hrishi072.repostealer.desktop"
        entry.write_text("[Desktop Entry]\nType=Application\nName=Repo Stealer\nComment=Encrypted Git capture and recovery\nExec=repo-stealer-desktop\nIcon=repo-stealer\nTerminal=false\nCategories=Development;\n", encoding="utf-8")
        subprocess.run(["desktop-file-validate", str(entry)], check=True)
        base, _, suffix = version.partition("-")
        spec = rpm / "SPECS/repo-stealer-desktop.spec"
        spec.write_text(f"""Name: repo-stealer-desktop
Version: {base}
Release: {"0." + suffix if suffix else "1"}
Summary: Encrypted repository capture and recovery desktop
License: LicenseRef-Proprietary
BuildArch: x86_64
Source0: repo-stealer-desktop
Source1: repo-stealer.png
Source2: com.hrishi072.repostealer.desktop
%global debug_package %{{nil}}
%description
Local encrypted Git history, vaults and same-key portable backups.
%install
install -D -m 0755 %{{SOURCE0}} %{{buildroot}}/usr/bin/repo-stealer-desktop
install -D -m 0644 %{{SOURCE1}} %{{buildroot}}/usr/share/icons/hicolor/128x128/apps/repo-stealer.png
install -D -m 0644 %{{SOURCE2}} %{{buildroot}}/usr/share/applications/com.hrishi072.repostealer.desktop
%files
/usr/bin/repo-stealer-desktop
/usr/share/icons/hicolor/128x128/apps/repo-stealer.png
/usr/share/applications/com.hrishi072.repostealer.desktop
""", encoding="utf-8")
        subprocess.run(["rpmbuild", "--define", f"_topdir {rpm}", "-bb", str(spec)], check=True)
        packages = list((rpm / "RPMS").rglob("*.rpm"))
        assert len(packages) == 1, "expected one native desktop RPM"
        shutil.copy2(packages[0], out / f"repo-stealer-desktop-{version}-linux-x86_64.rpm")
        count += 1
for path in bundle.rglob("*"):
    if path.is_file() and path.suffix in (".deb", ".rpm", ".AppImage", ".exe", ".msi"):
        if path.suffix == ".deb":
            package_name = subprocess.check_output(["dpkg-deb", "--field", str(path), "Package"], text=True).strip()
            assert package_name == "repo-stealer-desktop", "desktop package must coexist with the CLI"
        suffix = "-setup.exe" if path.suffix == ".exe" else path.suffix
        target = out / f"repo-stealer-desktop-{version}-{platform}-x86_64{suffix}"
        assert not target.exists(), "duplicate desktop package type"
        shutil.copyfile(path, target)
        count += 1
assert count >= (1 if os.name == "nt" else 3), "missing native desktop packages"
print("Prepared", count, "desktop release packages")
