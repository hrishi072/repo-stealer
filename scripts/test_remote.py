"""Synthetic HTTPS Git server and native encrypted capture acceptance tests."""
import argparse
import base64
import contextlib
import http.server
import hashlib
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
    def __init__(self, root, lfs=False):
        self.root = pathlib.Path(root)
        self.root.mkdir()
        self.source = fixture(self.root / "project.git")
        self.lfs_store = {}; self.lfs_source_objects = {}; self.lfs_mode = "normal"; self.lfs_calls = []
        self.lfs_delay=0.0;self.lfs_body_delay=0.0;self.lfs_metrics_lock=threading.Lock()
        self.lfs_active=0;self.lfs_peak=0;self.lfs_first=None;self.lfs_last=None;self.lfs_transfers=[]
        if lfs:
            first=b"historical encrypted LFS payload\x00"*4096
            oid=hashlib.sha256(first).hexdigest(); self.lfs_source_objects[oid]=first
            (self.source / "historical.bin").write_bytes((f"version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {len(first)}\n").encode("ascii"))
            git(self.source,"add",".");git(self.source,"commit","-m","historical LFS payload")
            (self.source / "historical.bin").unlink()
        # These are Git pointer/gitlink bytes only; neither external store is
        # available. Successful capture must never try to fetch either one.
        (self.source / "large.bin").write_bytes(("version https://git-lfs.github.com/spec/v1\noid sha256:" + "a" * 64 + "\nsize 10000000\n").encode("ascii"))
        if lfs:
            data=b"current encrypted LFS payload\x00"*2048
            oid=hashlib.sha256(data).hexdigest();self.lfs_source_objects[oid]=data
            (self.source/"large.bin").write_bytes((f"version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {len(data)}\n").encode("ascii"))
            (self.source/".lfsconfig").write_text("[lfs]\nurl = https://external.invalid/do-not-use\n")
            for oid,data in self.lfs_source_objects.items():
                path=self.source/".git/lfs/objects"/oid[:2]/oid[2:4]/oid;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
            self.lfs_store=dict(self.lfs_source_objects)
        (self.source / ".gitmodules").write_text('[submodule "component"]\npath = deps/component\nurl = https://external.invalid/component.git\n')
        git(self.source, "add", ".")
        head = git(self.source, "rev-parse", "HEAD").decode().strip()
        git(self.source, "update-index", "--add", "--cacheinfo", f"160000,{head},deps/component")
        git(self.source, "commit", "-m", "external-content pointers")
        if lfs:assert b"\r" not in git(self.source,"show","HEAD:large.bin"),"Fixture pointer must use canonical LF bytes"
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
        self.allow_receive = False
        self.require_auth = True
        self.mutate = False
        self.requests = []
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), self.handler())
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(server_certificate, self.key)
        self.server.socket = context.wrap_socket(self.server.socket, server_side=True)
        self.url = f"https://127.0.0.1:{self.server.server_port}/project.git"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def wait_lfs_idle(self,seconds=5):
        deadline=time.monotonic()+seconds
        while time.monotonic()<deadline:
            with self.lfs_metrics_lock:
                if self.lfs_active==0:return
            time.sleep(0.01)
        raise AssertionError("Synthetic LFS requests did not drain")

    def reset_lfs_metrics(self):
        with self.lfs_metrics_lock:
            assert self.lfs_active==0
            self.lfs_peak=0;self.lfs_first=None;self.lfs_last=None;self.lfs_transfers=[]

    @contextlib.contextmanager
    def lfs_transfer(self,method,oid):
        with self.lfs_metrics_lock:
            self.lfs_active+=1;self.lfs_peak=max(self.lfs_peak,self.lfs_active)
            if self.lfs_first is None:self.lfs_first=time.perf_counter()
            self.lfs_transfers.append((method,oid))
        try:
            time.sleep(self.lfs_delay)
            yield
        finally:
            with self.lfs_metrics_lock:
                self.lfs_active-=1;self.lfs_last=time.perf_counter()

    def handler(self):
        owner = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def send(self, code, data=b"", headers=None):
                # Cancellation/timeout closes sockets before either headers or
                # bodies may be sent. Both are expected fixture disconnects.
                with contextlib.suppress(BrokenPipeError, ConnectionResetError, ConnectionAbortedError, ssl.SSLError):
                    self.send_response(code)
                    self.send_header("Content-Length", str(len(data)))
                    self.send_header("Connection", "close")
                    for key, value in (headers or {}).items():
                        self.send_header(key, value)
                    self.end_headers()
                    self.wfile.write(data)

            def do_GET(self):
                self.process()

            def do_POST(self):
                self.process()

            def do_PUT(self):
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
                if path.startswith("/project.git/info/lfs/"):
                    owner.lfs_calls.append((self.command,path))
                    base=owner.lfs_url if hasattr(owner,"lfs_url") else owner.url
                    if path.endswith("/objects/batch"):
                        body=json.loads(self.rfile.read(int(self.headers.get("Content-Length","0"))))
                        assert body["transfers"]==["basic"] and body["operation"] in ("upload","download")
                        objects=[]
                        for obj in body["objects"]:
                            oid=obj["oid"];actions={}
                            if body["operation"]=="upload":
                                assert not getattr(owner,"actions",False),"Automation enabled during LFS upload"
                                if oid not in owner.lfs_store:actions={"upload":{"href":base+"/info/lfs/objects/"+oid},"verify":{"href":base+"/info/lfs/verify/"+oid}}
                            else:
                                if oid not in owner.lfs_store or owner.lfs_mode=="missing":objects.append({**obj,"error":{"code":404,"message":TOKEN}});continue
                                href=base+"/info/lfs/objects/"+oid
                                if owner.lfs_mode=="unsafe-url":href=href.replace("https:","http:")
                                actions={"download":{"href":href}}
                            row={**obj}
                            if actions:row["actions"]=actions
                            objects.append(row)
                        return self.send(200,json.dumps({"transfer":"basic","objects":objects}).encode())
                    oid=path.rsplit("/",1)[-1]
                    if "/verify/" in path:
                        body=json.loads(self.rfile.read(int(self.headers.get("Content-Length","0"))))
                        assert oid in owner.lfs_store and body["size"]==len(owner.lfs_store[oid])
                        return self.send(200)
                    with owner.lfs_transfer(self.command,oid):
                        if self.command=="PUT":
                            assert not getattr(owner,"actions",False)
                            body=self.rfile.read(int(self.headers["Content-Length"]))
                            if owner.lfs_mode=="upload-error":return self.send(503)
                            assert hashlib.sha256(body).hexdigest()==oid
                            owner.lfs_store[oid]=body;return self.send(200)
                        if owner.lfs_mode=="redirect":return self.send(302,headers={"Location":"https://external.invalid/never"})
                        data=owner.lfs_store[oid]
                        if owner.lfs_mode=="corrupt":data=b"x"*len(data)
                        if owner.lfs_mode=="extra":data+=b"x"
                        if owner.lfs_body_delay:
                            with contextlib.suppress(BrokenPipeError,ConnectionResetError,ConnectionAbortedError,ssl.SSLError):
                                self.send_response(200);self.send_header("Content-Length",str(len(data)));self.send_header("Connection","close");self.end_headers();self.wfile.flush()
                                time.sleep(owner.lfs_body_delay);self.wfile.write(data)
                            return
                        return self.send(200,data)
                if path.startswith("/dumb.git/"):
                    oid = git(owner.repository, "rev-parse", "HEAD").strip()
                    if path.endswith("/info/refs"):
                        return self.send(200, oid + b"\trefs/heads/main\n", {"Content-Type": "text/plain"})
                    if path.endswith("/HEAD"):
                        return self.send(200, b"ref: refs/heads/main\n")
                    if path.endswith("/objects/info/http-alternates"):
                        return self.send(200, (owner.url.replace("/project.git", "/alternate.git/objects/") + "\n").encode())
                    return self.send(404)
                allowed=("/project.git/info/refs", "/project.git/git-upload-pack", "/project.git/HEAD")
                if owner.allow_receive: allowed += ("/project.git/git-receive-pack",)
                if path not in allowed:
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


def probe_synthetic_git(server,work):
    """Anonymous, read-only clone probe after a controlled fixture failure.

    Never used against user hosts: this helper only accepts HTTPSFixture objects
    and does not send or print any Git/application credential.
    """
    assert isinstance(server,HTTPSFixture)
    url=server.lfs_url if hasattr(server,"lfs_url") else server.url
    assert urllib.parse.urlsplit(url).hostname=="127.0.0.1"
    env={k:v for k,v in os.environ.items() if not k.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1",GIT_CONFIG_GLOBAL=os.devnull,GIT_TERMINAL_PROMPT="0",TMP=str(work),TEMP=str(work),TMPDIR=str(work))
    old=server.require_auth;server.require_auth=False
    try:
        result=subprocess.run(["git","-c","credential.helper=","-c","core.hooksPath="+str(work/"unused-hooks"),"-c","protocol.allow=never","-c","protocol.https.allow=always","-c","http.followRedirects=false","-c","http.sslVerify=true","-c","http.schannelUseSSLCAInfo=true","-c","http.sslCAInfo="+str(server.certificate),"clone","--mirror","--no-local","--no-hardlinks","--ref-format=reftable",url,str(work/"synthetic-diagnostic.git")],env=env,capture_output=True,timeout=30)
        assert TOKEN.encode() not in result.stdout+result.stderr
        print("SYNTHETIC_ANONYMOUS_GIT_PROBE="+json.dumps({"returncode":result.returncode,"stderr":result.stderr.decode(errors="replace")},separators=(",",":")),flush=True)
    finally:server.require_auth=old


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
