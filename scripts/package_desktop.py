"""Name desktop assets distinctly from the CLI release assets."""
import json
import os
import pathlib
import shutil

root = pathlib.Path(__file__).resolve().parents[1]
desktop = root / "apps/desktop"
version = json.loads((desktop / "package.json").read_text())["version"]
bundle = desktop / "src-tauri/target/release/bundle"
out = root / "dist-desktop"
out.mkdir(exist_ok=True)
platform = "windows" if os.name == "nt" else "linux"
count = 0
for path in bundle.rglob("*"):
    if path.is_file() and path.suffix in (".deb", ".rpm", ".AppImage", ".exe", ".msi"):
        suffix = "-setup.exe" if path.suffix == ".exe" else path.suffix
        target = out / f"repo-stealer-desktop-{version}-{platform}-x86_64{suffix}"
        assert not target.exists(), "duplicate desktop package type"
        shutil.copyfile(path, target)
        count += 1
assert count >= (1 if os.name == "nt" else 3), "missing native desktop packages"
print("Prepared", count, "desktop release packages")
