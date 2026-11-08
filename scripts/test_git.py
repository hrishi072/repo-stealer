"""Full-history tests on real encrypted storage; no application verification bypass."""
import argparse
import ctypes
import os
import pathlib
import shutil
import subprocess
import tempfile
import time

PASSWORD = b"synthetic Git fixture passphrase\n"
MARKER = b"RS_GIT_CONTENT_SENTINEL_01_"


def app(binary, *args, ok=True):
    command = [str(binary), *map(str, args)]
    if args[0] in ("git", "vault"):
        command.append("--password-stdin")
    result = subprocess.run(command, input=PASSWORD, capture_output=True, timeout=180)
    assert (result.returncode == 0) == ok, (args, result.returncode, result.stdout, result.stderr)
    return result


def git(repo, *args, input=None):
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
    result = subprocess.run(["git", "-C", str(repo), "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false",
                             "-c", "core.hooksPath=" + str(repo / "unused-hooks"),
                             "-c", "user.name=Synthetic Fixture", "-c", "user.email=fixture@example.invalid", *args],
                            input=input, capture_output=True, env=env, timeout=120)
    assert result.returncode == 0, (args, result.stderr)
    return result.stdout


def inventory(repo):
    refs = git(repo, "for-each-ref", "--format=%(objectname)%09%(refname)%09%(symref)")
    head = subprocess.run(["git", "-C", str(repo), "symbolic-ref", "--quiet", "HEAD"], capture_output=True)
    head = b"symbolic:" + head.stdout if head.returncode == 0 else b"detached:" + git(repo, "rev-parse", "HEAD")
    objects = git(repo, "rev-list", "--objects", "--all", "--no-object-names", "HEAD") if refs or head.startswith(b"detached:") else b""
    return sorted(refs.splitlines()), head, sorted(set(objects.splitlines()))


def fixture(path, object_format="sha1"):
    path.mkdir()
    git(path, "init", "--initial-branch=main", "--object-format=" + object_format)
    (path / "history.txt").write_bytes(MARKER + b"first\n")
    git(path, "add", "."); git(path, "commit", "-m", "first")
    git(path, "checkout", "-b", "feature")
    (path / "feature.txt").write_text("feature\n")
    git(path, "add", "."); git(path, "commit", "-m", "feature")
    git(path, "checkout", "main")
    (path / "main.txt").write_text("main\n")
    git(path, "add", "."); git(path, "commit", "-m", "main")
    git(path, "merge", "--no-ff", "feature", "-m", "merge")
    git(path, "tag", "-a", "v1", "-m", "annotated tag")
    git(path, "tag", "lightweight")
    git(path, "notes", "add", "-m", "original note")
    git(path, "update-ref", "refs/remotes/origin/main", "HEAD")
    git(path, "symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main")
    # A valid Git name that would be a Windows filesystem reserved name.
    git(path, "update-ref", "refs/archive/portable", "HEAD")
    return path


def encrypted_files(root):
    if os.name != "nt":
        return  # Linux block verification plus raw LUKS scan happens in CI.
    get = ctypes.windll.kernel32.GetFileAttributesW
    get.argtypes = [ctypes.c_wchar_p]
    get.restype = ctypes.c_uint32
    for path in [root, *root.rglob("*")]:
        attributes = get(str(path.resolve()))
        assert attributes != 0xFFFFFFFF and attributes & 0x4000, f"EFS inheritance lost: {path}"


def restore_fixture(binary, backup, workspace, root):
    vault = root / "portable-vault"
    app(binary, "vault", "restore", backup, vault)
    destination = workspace / "cross-os.git"
    app(binary, "git", "restore", vault, "history", destination, "--work-root", workspace)
    git(destination, "fsck", "--full", "--strict", "--no-reflogs")
    assert git(destination, "show", "refs/heads/main:history.txt").startswith(MARKER)
    assert len(git(destination, "rev-list", "--all").splitlines()) == 5
    assert git(destination, "cat-file", "-t", "refs/tags/v1").strip() == b"tag"
    encrypted_files(workspace)


def wait_for(child, predicate, seconds=90):
    deadline = time.monotonic() + seconds
    while child.poll() is None and time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.005)
    stdout, stderr = child.communicate(timeout=30)
    raise AssertionError(("did not observe operation phase", child.returncode, stdout, stderr))


def start_capture(binary, vault, source, workspace, name):
    child = subprocess.Popen([str(binary), "git", "capture", str(vault), name, str(source),
                              "--work-root", str(workspace), "--password-stdin"],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    child.stdin.write(PASSWORD); child.stdin.close(); child.stdin = None
    return child


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("--fixture-out", type=pathlib.Path)
    parser.add_argument("--restore-fixture", type=pathlib.Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    parent = pathlib.Path(os.environ.get("RS_ENCRYPTED_PARENT", tempfile.gettempdir()))
    with tempfile.TemporaryDirectory(prefix="rs-git-tests-") as tmp, tempfile.TemporaryDirectory(prefix="rs-encrypted-test-", dir=parent) as encrypted_parent:
        root = pathlib.Path(tmp)
        workspace = pathlib.Path(encrypted_parent) / "workspace"
        app(binary, "workspace", "init", workspace)
        app(binary, "workspace", "check", workspace)
        ordinary = root / "ordinary"; ordinary.mkdir()
        app(binary, "workspace", "check", ordinary, ok=False)
        if args.restore_fixture:
            restore_fixture(binary, args.restore_fixture.resolve(), workspace, root)
            print("Cross-OS Git history restored on verified encrypted storage.")
            return
        vault = root / "vault"
        app(binary, "vault", "init", vault)
        source = fixture(root / "source with spaces")
        expected = inventory(source)
        app(binary, "git", "capture", vault, "history", source, "--work-root", workspace)
        destination = workspace / "restored.git"
        app(binary, "git", "restore", vault, "history", destination, "--work-root", workspace)
        assert inventory(destination) == expected
        assert git(destination, "config", "extensions.refStorage").strip() == b"reftable"
        app(binary, "git", "restore", vault, "history", destination, "--work-root", workspace, ok=False)
        existing = workspace / "existing"; existing.mkdir()
        app(binary, "git", "restore", vault, "history", existing, "--work-root", workspace, ok=False)
        app(binary, "git", "restore", vault, "history", root / "plaintext.git", "--work-root", workspace, ok=False)
        assert not (root / "plaintext.git").exists()
        # Detached HEAD, then empty/unborn and SHA-256 repositories.
        git(source, "checkout", "--detach", "HEAD~1")
        app(binary, "git", "capture", vault, "detached", source, "--work-root", workspace)
        app(binary, "git", "restore", vault, "detached", workspace / "detached.git", "--work-root", workspace)
        assert inventory(source) == inventory(workspace / "detached.git")
        empty = root / "empty"; empty.mkdir(); git(empty, "init", "--initial-branch=unborn")
        app(binary, "git", "capture", vault, "empty", empty, "--work-root", workspace)
        app(binary, "git", "restore", vault, "empty", workspace / "empty.git", "--work-root", workspace)
        assert inventory(empty) == inventory(workspace / "empty.git")
        sha = fixture(root / "sha256", "sha256")
        app(binary, "git", "capture", vault, "sha256", sha, "--work-root", workspace)
        app(binary, "git", "restore", vault, "sha256", workspace / "sha256.git", "--work-root", workspace)
        assert inventory(sha) == inventory(workspace / "sha256.git")
        # Shallow and promisor sources must never be labeled complete.
        shallow = root / "shallow"
        git(root, "clone", "--depth=1", source.as_uri(), str(shallow))
        app(binary, "git", "capture", vault, "shallow", shallow, "--work-root", workspace, ok=False)
        git(sha, "config", "remote.origin.promisor", "true")
        app(binary, "git", "capture", vault, "partial", sha, "--work-root", workspace, ok=False)
        # Captured snapshots, including Git history, remain portable vault data.
        backup = root / "git.rsbackup"
        app(binary, "vault", "backup", vault, backup)
        if args.fixture_out:
            args.fixture_out.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(backup, args.fixture_out)
        # Fail a capture when refs mutate after its manifest is frozen.
        git(source, "checkout", "main")
        old = git(source, "rev-parse", "HEAD").strip().decode()
        (source / "large.bin").write_bytes(os.urandom(32 * 1024 * 1024))
        git(source, "add", "large.bin"); git(source, "commit", "-m", "large fixture")
        large_oid = git(source, "rev-parse", "HEAD").strip().decode()
        child = start_capture(binary, vault, source, workspace, "changed")
        wait_for(child, lambda: any(workspace.glob(".rs-git-*/source-view.git")))
        git(source, "update-ref", "refs/heads/main", old)
        stdout, stderr = child.communicate(timeout=180)
        assert child.returncode != 0 and b"changed during capture" in stderr, (stdout, stderr)
        git(source, "update-ref", "refs/heads/main", large_oid)
        # Kill only after Git subprocesses finish and vault encryption begins.
        # Any remaining plaintext scratch is still inside encrypted storage.
        objects = vault / "data/objects"
        before = set(objects.iterdir())
        child = start_capture(binary, vault, source, workspace, "killed")
        wait_for(child, lambda: bool(set(objects.iterdir()) - before))
        child.kill(); child.communicate(timeout=30)
        assert b"killed\t" not in app(binary, "vault", "list", vault).stdout
        app(binary, "vault", "verify", vault)
        encrypted_files(workspace)
        # Vault files expose neither Git content nor original logical names.
        for path in vault.rglob("*"):
            if path.is_file():
                data = path.read_bytes()
                assert MARKER not in data and b"refs/heads/main" not in data
        assert inventory(destination) == expected
        print("Encrypted Git roundtrips, refs/HEAD/object IDs, SHA-256, empty/detached, rejection, mutation and crash tests passed.")


if __name__ == "__main__":
    main()
