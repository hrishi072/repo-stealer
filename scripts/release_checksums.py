import hashlib
import json
import os
import pathlib
import subprocess

metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
versions = {package["version"] for package in metadata["packages"]}
assert len(versions) == 1, "workspace package versions differ"
version = versions.pop()
assert os.environ["RELEASE_TAG"] == "v" + version, "tag and Cargo version differ"
paths = sorted(pathlib.Path("dist").iterdir())
assert any(p.suffix == ".exe" for p in paths)
for suffix in (".deb", ".rpm", ".AppImage", ".zip"):
    assert any(p.suffix == suffix for p in paths), f"missing {suffix}"
pathlib.Path("dist/SHA256SUMS").write_text("".join(
    f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in paths), encoding="utf-8")
