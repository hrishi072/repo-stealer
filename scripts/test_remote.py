"""Synthetic HTTPS Git server and native encrypted capture acceptance tests."""
import argparse
import base64
import contextlib
import http.server
import json
import os
import pathlib
import shutil
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.parse

from test_git import PASSWORD, encrypted_files, fixture, git, inventory

TOKEN = "synthetic token @:$`never-log-this"
USERNAME = "fixture"


def openssl():
    found = shutil.which("openssl")
    if found:
        return found
    root = pathlib.Path(subprocess.check_output(["git", "--exec-path"], text=True).strip()).parents[2]
    for candidate in [root / "usr/bin/openssl.exe", root / "mingw64/bin/openssl.exe"]:
        if candidate.is_file():
            return str(candidate)
    raise RuntimeError("OpenSSL is required to generate the disposable HTTPS fixture")


class HTTPSFixture:
    def __init__(self, root):
        self.root = pathlib.Path(root)
        self.root.mkdir()
        self.source = fixture(self.root / "project.git")
        # These are Git pointer/gitlink bytes only; neither external store is
        # available. Successful capture must never try to fetch either one.
        (self.source / "large.bin").write_text("version https://git-lfs.github.com/spec/v1\noid sha256:" + "a" * 64 + "\nsize 10000000\n")
        (self.source / ".gitmodules").write_text('[submodule "component"]\npath = deps/component\nurl = https://external.invalid/component.git\n')
        git(self.source, "add", ".")
        head = git(self.source, "rev-parse", "HEAD").decode().strip()
        git(self.source, "update-index", "--add", "--cacheinfo", f"160000,{head},deps/component")
        git(self.source, "commit", "-m", "external-content pointers")
        # Export an ordinary repository over HTTP without changing its working
        # tree; the server backend needs the bare directory at this URL.
        self.export = self.root / "export"
        self.export.mkdir()
        git(self.source, "clone", "--mirror", str(self.source), str(self.export / "project.git"))
        self.repository = self.export / "project.git"
        self.certificate = self.root / "ca.pem"
        self.key = self.root / "server.key"
        owner = self
        crl_route = "/" + os.urandom(8).hex() + ".crl"
        self.crl_requests = 0
        class RevocationHandler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_GET(self):
                assert self.headers.get("Authorization") is None, "Git credentials reached CRL endpoint"
                if self.path != crl_route:
                    self.send_error(404)
                    return
                owner.crl_requests += 1
                data = (owner.root / "fixture.crl").read_bytes()
                self.send_response(200)
                self.send_header("Content-Type", "application/pkix-crl")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
        self.revocation = http.server.ThreadingHTTPServer(("127.0.0.1", 0), RevocationHandler)
        self.revocation_thread = threading.Thread(target=self.revocation.serve_forever, daemon=True)
        ca_key = self.root / "ca.key"
        server_certificate = self.root / "server.pem"
        request = self.root / "server.csr"
        extensions = self.root / "server.ext"
        extensions.write_text("basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\nauthorityKeyIdentifier=keyid,issuer\nsubjectKeyIdentifier=hash\n" + f"crlDistributionPoints=URI:http://127.0.0.1:{self.revocation.server_port}{crl_route}\n")
        (self.root / "index.txt").write_text("")
        (self.root / "crlnumber").write_text("01\n")
        (self.root / "ca.conf").write_text("[ca]\ndefault_ca=fixture_ca\n[fixture_ca]\ndatabase=index.txt\nprivate_key=ca.key\ncertificate=ca.pem\ndefault_md=sha256\ndefault_crl_days=2\ncrlnumber=crlnumber\ncrl_extensions=crl_ext\n[crl_ext]\nauthorityKeyIdentifier=keyid:always\n")
        # A CA certificate is not a valid TLS end-entity certificate. Keep a
        # separate signed leaf so both Git and strict Rustls verification pass.
        commands = [
            ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=Repo Stealer fixture CA", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-addext", "subjectKeyIdentifier=hash", "-keyout", str(ca_key), "-out", str(self.certificate)],
            ["req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost", "-keyout", str(self.key), "-out", str(request)],
            ["x509", "-req", "-in", str(request), "-CA", str(self.certificate), "-CAkey", str(ca_key), "-CAcreateserial", "-days", "2", "-extfile", str(extensions), "-out", str(server_certificate)],
        ]
        commands.extend([
            ["ca", "-gencrl", "-config", "ca.conf", "-out", "fixture.crl.pem"],
            ["crl", "-in", "fixture.crl.pem", "-outform", "DER", "-out", "fixture.crl"],
        ])
        for arguments in commands:
            subprocess.run([openssl(), *arguments], cwd=self.root, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        backend = pathlib.Path(subprocess.check_output(["git", "--exec-path"], text=True).strip()) / "git-http-backend"
        self.backend = backend if backend.is_file() else backend.with_suffix(".exe")
        assert self.backend.is_file()
        self.require_auth = True
        self.mutate = False
        self.requests = []
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), self.handler())
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(server_certificate, self.key)
        self.server.socket = context.wrap_socket(self.server.socket, server_side=True)
        self.url = f"https://127.0.0.1:{self.server.server_port}/project.git"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def handler(self):
        owner = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def send(self, code, data=b"", headers=None):
                self.send_response(code)
                self.send_header("Content-Length", str(len(data)))
                self.send_header("Connection", "close")
                for key, value in (headers or {}).items():
                    self.send_header(key, value)
                self.end_headers()
                with contextlib.suppress(BrokenPipeError, ConnectionResetError, ssl.SSLError):
                    self.wfile.write(data)

            def do_GET(self):
                self.process()

            def do_POST(self):
                self.process()

            def process(self):
                parsed = urllib.parse.urlsplit(self.path)
                path = parsed.path
                owner.requests.append((self.command, path))
                if path.startswith("/redirect.git/"):
                    return self.send(302, headers={"Location": owner.url + path.removeprefix("/redirect.git")})
                if path.startswith("/slow.git/"):
                    time.sleep(4)
                    return self.send(503)
                expected = "Basic " + base64.b64encode(f"{USERNAME}:{TOKEN}".encode()).decode()
                if owner.require_auth and self.headers.get("Authorization") != expected:
                    return self.send(401, headers={"WWW-Authenticate": 'Basic realm="synthetic Git"'})
                if path.startswith("/dumb.git/"):
                    oid = git(owner.repository, "rev-parse", "HEAD").strip()
                    if path.endswith("/info/refs"):
                        return self.send(200, oid + b"\trefs/heads/main\n", {"Content-Type": "text/plain"})
                    if path.endswith("/HEAD"):
                        return self.send(200, b"ref: refs/heads/main\n")
                    if path.endswith("/objects/info/http-alternates"):
                        return self.send(200, (owner.url.replace("/project.git", "/alternate.git/objects/") + "\n").encode())
                    return self.send(404)
                if path not in ("/project.git/info/refs", "/project.git/git-upload-pack", "/project.git/HEAD"):
                    return self.send(404)
                length = int(self.headers.get("Content-Length", "0"))
                if length > 1024 * 1024:
                    return self.send(413)
                body = self.rfile.read(length)
                env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
                env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                           GIT_PROJECT_ROOT=str(owner.export), GIT_HTTP_EXPORT_ALL="1",
                           PATH_INFO=path, QUERY_STRING=parsed.query, REQUEST_METHOD=self.command,
                           CONTENT_TYPE=self.headers.get("Content-Type", ""), CONTENT_LENGTH=str(length),
                           REMOTE_ADDR="127.0.0.1", SERVER_PROTOCOL="HTTP/1.1")
                if self.headers.get("Git-Protocol"):
                    env["GIT_PROTOCOL"] = self.headers["Git-Protocol"]
                response = subprocess.run([str(owner.backend)], input=body, capture_output=True, env=env, timeout=60)
                assert response.returncode == 0, response.stderr
                header, separator, data = response.stdout.partition(b"\r\n\r\n")
                if not separator:
                    header, separator, data = response.stdout.partition(b"\n\n")
                assert separator, "Malformed CGI fixture response"
                fields = dict(line.decode().split(": ", 1) for line in header.splitlines())
                status = int(fields.pop("Status", "200 OK").split()[0])
                if owner.mutate and b"command=fetch" in body:
                    owner.mutate = False
                    git(owner.repository, "update-ref", "refs/heads/changed-during-capture", "HEAD")
                self.send(status, data, fields)

        return Handler

    def __enter__(self):
        self.revocation_thread.start()
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        self.revocation.shutdown()
        self.revocation.server_close()
        self.revocation_thread.join(timeout=5)


def command(binary, *args, secrets=None, ok=True):
    data = json.dumps(secrets).encode() if secrets is not None else PASSWORD
    option = "--secrets-stdin" if secrets is not None else "--password-stdin"
    argv = [str(binary), *map(str, args)]
    if args[0] != "workspace":
        argv.append(option)
    result = subprocess.run(argv, input=data, capture_output=True, timeout=180)
    assert (result.returncode == 0) == ok, (args, result.returncode, result.stdout, result.stderr)
    assert TOKEN.encode() not in result.stdout + result.stderr, "Authentication token entered diagnostics"
    return result


def manifest(binary, vault, name, root):
    output = root / (name + ".snapshot")
    command(binary, "vault", "export", vault, name, output, "--plaintext")
    with output.open("rb") as source:
        assert source.read(8) == b"RSGIT001"
        length = int.from_bytes(source.read(4), "little")
        assert 0 < length <= 8 * 1024 * 1024
        metadata = json.loads(source.read(length))
    assert metadata["version"] == 2
    assert TOKEN not in json.dumps(metadata)
    assert metadata["remote"]["inventory"]["headSubmodules"] == 1
    assert metadata["remote"]["inventory"]["headLfsPointers"] == 1
    assert metadata["remote"]["inventory"]["lfsScanComplete"]
    return metadata


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=pathlib.Path, nargs="?")
    parser.add_argument("--probe-https", action="store_true", help="Diagnose only synthetic anonymous Git TLS; no app/vault or credential access")
    parser.add_argument("--fixture-out", type=pathlib.Path)
    parser.add_argument("--restore-fixture", type=pathlib.Path)
    args = parser.parse_args()
    if args.probe_https:
        with tempfile.TemporaryDirectory(prefix="rs-tls-probe-") as tmp, HTTPSFixture(pathlib.Path(tmp) / "https") as server:
            server.require_auth = False
            env = {key:value for key,value in os.environ.items() if not key.startswith("GIT_")}
            env.update(GIT_CONFIG_NOSYSTEM="1",GIT_CONFIG_GLOBAL=os.devnull)
            result=subprocess.run(["git","-c","http.sslVerify=true","-c","http.schannelUseSSLCAInfo=true","-c","http.sslCAInfo="+str(server.certificate),"-c","protocol.version=2","ls-remote",server.url],cwd=server.root,env=env,capture_output=True,timeout=30)
            # This server is synthetic and anonymous: diagnostics contain no
            # application credentials, unlike production Git worker output.
            assert result.returncode==0, result.stderr.decode(errors="replace")
            if os.name=="nt":
                assert server.crl_requests>0, "Schannel did not check the fixture CRL"
            print("Anonymous synthetic Git HTTPS certificate-chain probe passed.")
        return
    if args.binary is None:
        parser.error("binary is required unless --probe-https is selected")
    binary = args.binary.resolve()
    parent = pathlib.Path(os.environ.get("RS_ENCRYPTED_PARENT", tempfile.gettempdir()))
    with tempfile.TemporaryDirectory(prefix="rs-remote-") as temporary, tempfile.TemporaryDirectory(prefix="rs-remote-encrypted-", dir=parent) as encrypted:
        root = pathlib.Path(temporary)
        work = pathlib.Path(encrypted) / "workspace"
        command(binary, "workspace", "init", work)
        vault = root / "vault"
        if args.restore_fixture:
            command(binary, "vault", "restore", args.restore_fixture.resolve(), vault)
            metadata = manifest(binary, vault, "remote-history", root)
            destination = work / "cross-os.git"
            command(binary, "git", "restore", vault, "remote-history", destination, "--work-root", work)
            assert git(destination, "show", "HEAD:large.bin").startswith(b"version https://git-lfs.github.com/spec/v1")
            assert metadata["remote"]["advertisedRefCount"] > 0
            encrypted_files(work)
            print("Cross-OS authenticated remote provenance and Git history recovery passed.")
            return
        command(binary, "vault", "init", vault)
        secrets = {"vault_passphrase": PASSWORD.decode().strip(), "token": TOKEN}
        with HTTPSFixture(root / "https") as server:
            def capture(name, url=None, *, token=TOKEN, ca=True, ok=True, timeout=30):
                auth = ["--username", USERNAME] if token is not None else []
                trust = ["--ca-file", server.certificate] if ca else []
                return command(binary, "git", "capture-remote", vault, name, url or server.url,
                               "--work-root", work, "--timeout-seconds", timeout, *auth, *trust,
                               secrets={**secrets, "token": token}, ok=ok)
            capture("untrusted-certificate", ca=False, ok=False)
            capture("bad-token", token="incorrect synthetic token", ok=False)
            capture("remote-history")
            metadata = manifest(binary, vault, "remote-history", root)
            assert metadata["remote"]["url"] == server.url
            destination = work / "restored.git"
            command(binary, "git", "restore", vault, "remote-history", destination, "--work-root", work)
            assert inventory(server.repository) == inventory(destination)
            if os.name != "nt":
                assert destination.stat().st_mode & 0o077 == 0, "Restored repository is not owner-private"
            count = len(server.requests)
            capture("remote-history", ok=False)
            assert len(server.requests) == count, "Duplicate snapshot made unnecessary network requests"
            capture("redirect-refused", server.url.replace("/project.git", "/redirect.git"), ok=False)
            assert server.requests[-1][1].startswith("/redirect.git/"), "Redirect was followed"
            server.require_auth = False
            capture("anonymous", token=None)
            capture("dumb-alternate", server.url.replace("/project.git", "/dumb.git"), token=None, ok=False)
            assert not any(path.startswith("/alternate.git/") for _, path in server.requests), "HTTP alternate was followed"
            server.require_auth = True
            server.mutate = True
            failed = capture("changed-remote", ok=False)
            assert b"Remote refs or HEAD changed" in failed.stderr
            capture("timeout", server.url.replace("/project.git", "/slow.git"), timeout=1, ok=False)
            assert not any("git-receive-pack" in path or "external.invalid" in path for _, path in server.requests)
        listing = command(binary, "vault", "list", vault).stdout
        for rejected in (b"untrusted-certificate", b"bad-token", b"redirect-refused", b"dumb-alternate", b"changed-remote", b"timeout"):
            assert rejected not in listing, rejected
        encrypted_files(work)
        for path in vault.rglob("*"):
            if path.is_file() and path.name != "lock":
                data = path.read_bytes()
                assert TOKEN.encode() not in data and server.url.encode() not in data
        if args.fixture_out:
            output = args.fixture_out.resolve()
            output.parent.mkdir(parents=True, exist_ok=True)
            command(binary, "vault", "backup", vault, output)
        print("HTTPS full-history, auth/custom CA, provenance, inventory, no-clobber, redirect/alternate refusal, mutation and timeout checks passed.")


if __name__ == "__main__":
    main()
