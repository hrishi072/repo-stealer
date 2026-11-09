"""Install the CI desktop package and launch its actual executable via WebDriver."""
import os
import pathlib
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
packages = root / "dist-desktop"
harness = root / "scripts/test_desktop_native.py"


def smoke(application):
    subprocess.run([sys.executable, str(harness), str(application), "--smoke"], check=True, timeout=180)


if os.name == "nt":
    installers = list(packages.glob("*-setup.exe"))
    assert len(installers) == 1
    with tempfile.TemporaryDirectory(prefix="rs-installed-ui-", dir=os.environ["RUNNER_TEMP"]) as temporary:
        destination = pathlib.Path(temporary) / "application"
        subprocess.run([str(installers[0]), "/S", f"/D={destination}"], check=True, timeout=180)
        application = destination / "repo-stealer-desktop.exe"
        assert application.is_file(), "NSIS did not install the desktop executable"
        try:
            smoke(application)
        finally:
            uninstaller = destination / "uninstall.exe"
            assert uninstaller.is_file(), "NSIS did not install an uninstaller"
            subprocess.run([str(uninstaller), "/S", f"_?={destination}"], check=True, timeout=180)
else:
    debs = list(packages.glob("*.deb"))
    rpms = list(packages.glob("*.rpm"))
    images = list(packages.glob("*.AppImage"))
    assert len(debs) == len(rpms) == len(images) == 1
    subprocess.run(["sudo", "dpkg", "-i", str(debs[0])], check=True, timeout=120)
    smoke("/usr/bin/repo-stealer-desktop")
    subprocess.run(["rpm", "--checksig", str(rpms[0])], check=True)
    assert subprocess.check_output(["rpm", "-qp", "--queryformat", "%{NAME}", str(rpms[0])], text=True) == "repo-stealer-desktop"
    # Extraction avoids requiring FUSE in CI while exercising bundled libraries
    # and AppRun. Only the disposable extraction directory is removed.
    with tempfile.TemporaryDirectory(prefix="rs-appimage-") as temporary:
        image = images[0]
        image.chmod(image.stat().st_mode | 0o100)
        subprocess.run([str(image), "--appimage-extract"], cwd=temporary, stdout=subprocess.DEVNULL, check=True, timeout=120)
        smoke(pathlib.Path(temporary) / "squashfs-root/AppRun")
print("Desktop installer and native launch checks passed.")
