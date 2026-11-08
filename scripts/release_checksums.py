import hashlib
import os
import pathlib
import tomllib

version = tomllib.loads(pathlib.Path("Cargo.toml").read_text())["workspace"]["package"]["version"]
assert os.environ["RELEASE_TAG"] == "v" + version, "tag and Cargo version differ"
paths = sorted(pathlib.Path("dist").iterdir())
assert any(p.suffix == ".exe" for p in paths)
for suffix in (".deb", ".rpm", ".AppImage", ".zip"):
    assert any(p.suffix == suffix for p in paths), f"missing {suffix}"
pathlib.Path("dist/SHA256SUMS").write_text("".join(
    f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in paths), encoding="utf-8")
