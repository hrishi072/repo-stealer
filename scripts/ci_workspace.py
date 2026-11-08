"""CI-only LUKS fixture. Formats an exclusively created regular image, never a disk."""
import argparse
import mmap
import os
import pathlib
import shutil
import subprocess
import tempfile
import uuid

MARKER = b"RS_GIT_CONTENT_SENTINEL_01_"


def call(*args):
    subprocess.run(list(map(str, args)), check=True)


def destroy(root, mapping):
    root = pathlib.Path(root)
    assert root.name.startswith("rs-luks-ci-") and mapping.startswith("rs-git-")
    if subprocess.run(["mountpoint", "-q", str(root / "mounted")]).returncode == 0:
        call("sudo", "umount", root / "mounted")
    if pathlib.Path("/dev/mapper", mapping).exists():
        call("sudo", "cryptsetup", "close", mapping)
    image = root / "encrypted.img"
    if image.exists():
        with image.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as raw:
            assert raw.find(MARKER) == -1, "plaintext marker found on raw encrypted storage"
        print("Unmounted LUKS image contains no plaintext Git content marker.")
    shutil.rmtree(root)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("operation", choices=["create", "destroy"])
    args = parser.parse_args()
    assert os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("RUNNER_OS") == "Linux", "CI fixture provisioning only"
    if args.operation == "destroy":
        if os.environ.get("RS_LUKS_ROOT"):
            destroy(os.environ["RS_LUKS_ROOT"], os.environ["RS_LUKS_MAPPING"])
        return
    root = pathlib.Path(tempfile.mkdtemp(prefix="rs-luks-ci-", dir=os.environ["RUNNER_TEMP"]))
    mapping = "rs-git-" + uuid.uuid4().hex[:12]
    image = root / "encrypted.img"
    with image.open("xb") as stream:
        stream.truncate(256 * 1024 * 1024)
    key = root / "ci-only.key"
    key.write_bytes(os.urandom(32))
    key.chmod(0o600)
    mounted = root / "mounted"
    mounted.mkdir()
    try:
        call("sudo", "cryptsetup", "luksFormat", "--batch-mode", "--type", "luks2", "--pbkdf", "pbkdf2", "--iter-time", "100", "--key-file", key, image)
        call("sudo", "cryptsetup", "open", "--key-file", key, image, mapping)
        call("sudo", "mkfs.ext4", "-q", "/dev/mapper/" + mapping)
        call("sudo", "mount", "/dev/mapper/" + mapping, mounted)
        call("sudo", "chown", f"{os.getuid()}:{os.getgid()}", mounted)
        with (mounted / "encryption-proof").open("wb") as proof:
            proof.write(MARKER * 100)
            proof.flush()
            os.fsync(proof.fileno())
        with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as env:
            env.write(f"RS_ENCRYPTED_PARENT={mounted}\nRS_LUKS_ROOT={root}\nRS_LUKS_MAPPING={mapping}\n")
    except BaseException:
        destroy(root, mapping)
        raise


if __name__ == "__main__":
    main()
