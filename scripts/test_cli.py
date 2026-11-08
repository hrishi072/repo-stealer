"""Native CLI and cross-OS archive checks. Test passphrase/data are synthetic."""
import argparse
import pathlib
import shutil
import subprocess
import tempfile
import time

PASSWORD = b"synthetic CI passphrase only\n"
MARKER = b"private-synthetic-content-42"
NAME = "private synthetic name"
PAYLOAD = MARKER * 6000


def run(binary, *args, ok=True):
    p = subprocess.run([str(binary), "vault", *map(str, args), "--password-stdin"],
                       input=PASSWORD, capture_output=True, timeout=90)
    assert (p.returncode == 0) == ok, (args, p.returncode, p.stdout, p.stderr)
    return p


def ciphertext_only(root):
    for path in root.rglob("*"):
        if path.is_file():
            data = path.read_bytes()
            for secret in (MARKER, NAME.encode(), b"AGE-SECRET-KEY-", PASSWORD.strip()):
                assert secret not in data, f"plaintext leaked into {path.name}"


def check_restore(binary, backup, root):
    output = root / "restored vault"
    run(binary, "restore", backup, output)
    run(binary, "verify", output)
    plaintext = root / "explicit export.bin"
    run(binary, "export", output, NAME, plaintext, "--plaintext")
    assert plaintext.read_bytes() == PAYLOAD
    run(binary, "export", output, NAME, plaintext, "--plaintext", ok=False)
    ciphertext_only(output)
    return output


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("--fixture-out", type=pathlib.Path)
    parser.add_argument("--restore-fixture", type=pathlib.Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    with tempfile.TemporaryDirectory(prefix="repo-stealer-tests-") as tmp:
        root = pathlib.Path(tmp)
        if args.restore_fixture:
            check_restore(binary, args.restore_fixture.resolve(), root)
            print("Cross-platform fixture restored and verified.")
            return
        vault = root / "vault with spaces"
        source = root / "input.bin"
        source.write_bytes(PAYLOAD)
        run(binary, "init", vault)
        run(binary, "put", vault, NAME, source)
        run(binary, "put", vault, "empty", root / "missing", ok=False)
        assert NAME.encode() in run(binary, "list", vault).stdout
        ciphertext_only(vault)
        backup = root / "portable.rsbackup"
        run(binary, "backup", vault, backup)
        run(binary, "verify-backup", backup)
        assert MARKER not in backup.read_bytes()
        shutil.rmtree(vault)  # proves standalone recovery without original vault
        restored = check_restore(binary, backup, root)
        run(binary, "restore", backup, restored, ok=False)
        bad = root / "truncated.rsbackup"
        bad.write_bytes(backup.read_bytes()[:-1])
        missing = root / "must-not-exist"
        run(binary, "restore", bad, missing, ok=False)
        assert not missing.exists()

        # Kill a process after it has started an encrypted write. Only the
        # explicitly supplied source file contains plaintext outside the vault.
        big = root / "large-input.bin"
        with big.open("wb") as stream:
            block = MARKER * 40000
            for _ in range(128):
                stream.write(block)
        child = subprocess.Popen([str(binary), "vault", "put", str(restored),
                                  "interrupted", str(big), "--password-stdin"],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        child.stdin.write(PASSWORD)
        child.stdin.close()
        deadline = time.monotonic() + 30
        existing = set((restored / "data/objects").iterdir())
        started = False
        while time.monotonic() < deadline and child.poll() is None:
            if set((restored / "data/objects").iterdir()) - existing:
                started = True
                child.kill()
                break
            time.sleep(0.005)
        child.wait(timeout=30)
        assert started, "could not observe encrypted write before termination"
        ciphertext_only(restored)
        run(binary, "verify", restored)
        assert b"interrupted" not in run(binary, "list", restored).stdout
        if args.fixture_out:
            args.fixture_out.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(backup, args.fixture_out)
        print("CLI roundtrip, corruption, no-clobber, plaintext scan and killed-writer recovery passed.")


if __name__ == "__main__":
    main()
