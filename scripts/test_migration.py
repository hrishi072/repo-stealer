"""Read-only API/Git preflight against synthetic TLS providers, on real LUKS/EFS."""
import argparse
import contextlib
import ssl
import io
import json
import os
import pathlib
import tempfile
import time
import urllib.parse

from test_git import PASSWORD, encrypted_files, fixture, git
from test_remote import HTTPSFixture, TOKEN, USERNAME, command


class ProviderFixture(HTTPSFixture):
    def __init__(self, root):
        self.provider = "github"
        self.prefix = ""
        self.mode = "normal"
        self.repo_reads = 0
        self.policy_pages = []
        self.external_requests = []
        self.rpc_bodies = []
        self.allow_fetch = False
        super().__init__(root)

    @property
    def instance(self):
        return self.url.removesuffix("/project.git") + self.prefix

    @property
    def project(self):
        return "owner/repo" if self.provider == "github" else "group/sub/repo"

    def handler(self):
        parent = super().handler()
        owner = self

        class Handler(parent):
            def process(self):
                parsed = urllib.parse.urlsplit(self.path)
                path = parsed.path
                owner.external_requests.append((self.command, path))
                api = owner.prefix + ("/api/v3/" if owner.provider == "github" else "/api/v4/")
                if path.startswith(api):
                    assert self.command == "GET", "Provider API mutation attempted"
                    if owner.mode == "slow-api-body":
                        with contextlib.suppress(BrokenPipeError, ConnectionResetError, ConnectionAbortedError, ssl.SSLError):
                            self.send_response(200)
                            self.send_header("Content-Length", "2")
                            self.send_header("Connection", "close")
                            self.end_headers()
                            self.wfile.write(b"{"); self.wfile.flush()
                            time.sleep(4)
                            self.wfile.write(b"}")
                        return
                    if owner.mode == "slow-api":
                        time.sleep(4)
                        return self.send(503)
                    if owner.mode == "redirect":
                        return self.send(302, headers={"Location": owner.instance + "/redirect-trap"})
                    if self.headers.get("Authorization") != "Bearer " + TOKEN:
                        return self.send(401, b"must never echo provider error body " + TOKEN.encode())
                    if owner.mode in ("missing", "denied", "rate"):
                        code = {"missing": 404, "denied": 403, "rate": 429}[owner.mode]
                        return self.send(code, TOKEN.encode(), {"Retry-After": "10"} if code == 429 else {})
                    if owner.mode == "invalid-json":
                        return self.send(200, b"not json " + TOKEN.encode())
                    if owner.mode == "oversized":
                        return self.send(200, b" " * (2 * 1024 * 1024 + 1))
                    if path == api + "user":
                        return self.send(200, json.dumps({"id": 10, "login" if owner.provider == "github" else "username": USERNAME}).encode())
                    repo_path = api + ("repos/" + owner.project if owner.provider == "github" else "projects/" + urllib.parse.quote(owner.project, safe=""))
                    if path == repo_path:
                        owner.repo_reads += 1
                        clone = owner.instance + "/" + owner.project + ".git"
                        if owner.mode == "hostile-clone":
                            clone = "https://external.invalid/owner/repo.git"
                        repo = {"id": 21, "archived": owner.mode == "archived", "default_branch": "main"}
                        if owner.provider == "github":
                            repo.update(full_name=owner.project, clone_url=clone, disabled=False, permissions={"push": owner.mode != "read-role"})
                        else:
                            repo.update(path_with_namespace=owner.project, http_url_to_repo=clone, permissions={"project_access": {"access_level": 30}, "group_access": None})
                        if owner.mode == "changed-identity" and owner.repo_reads == 2:
                            repo["id"] = 22
                        if owner.mode == "changed-refs" and owner.repo_reads == 2:
                            git(owner.repository, "update-ref", "refs/heads/changed-in-preflight", "HEAD")
                        return self.send(200, json.dumps(repo).encode())
                    if path.startswith(repo_path + "/"):
                        page = int(urllib.parse.parse_qs(parsed.query)["page"][0])
                        owner.policy_pages.append((path, page))
                        if owner.mode == "policy-denied":
                            return self.send(403, TOKEN.encode())
                        ruleset = path.endswith("/rulesets")
                        items = [{"id": 1}] if ruleset else [{"name": "main"}]
                        headers = {}
                        if owner.mode == "pagination" and page == 1:
                            headers["Link"] = '<https://external.invalid/steal-token>; rel="next"'
                        if page > 1:
                            items = []
                        return self.send(200, json.dumps(items).encode(), headers)
                    return self.send(404)
                external = owner.prefix + "/" + owner.project + ".git"
                if path.startswith(external + "/"):
                    if self.command == "POST":
                        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
                        owner.rpc_bodies.append(body)
                        if not owner.allow_fetch:
                            assert b"command=fetch" not in body and b"command=push" not in body, "Plan fetched or pushed objects"
                        self.rfile = io.BytesIO(body)
                    self.path = self.path.replace(external, "/project.git", 1)
                    if owner.mode == "git-denied":
                        return self.send(403)
                    return super().process()
                if path.startswith("/project.git/"):
                    return super().process()  # existing remote capture fixture route
                return self.send(404)

        return Handler


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=pathlib.Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    parent = pathlib.Path(os.environ.get("RS_ENCRYPTED_PARENT", tempfile.gettempdir()))
    with tempfile.TemporaryDirectory(prefix="rs-plan-") as tmp, tempfile.TemporaryDirectory(prefix="rs-plan-encrypted-", dir=parent) as encrypted:
        root = pathlib.Path(tmp)
        work = pathlib.Path(encrypted) / "workspace"
        command(binary, "workspace", "init", work)
        vault = root / "vault"
        command(binary, "vault", "init", vault)
        secrets = {"vault_passphrase": PASSWORD.decode().strip(), "token": TOKEN}
        with ProviderFixture(root / "https") as server:
            command(binary, "git", "capture", vault, "source", server.source, "--work-root", work)
            server.allow_fetch = True
            command(binary, "git", "capture-remote", vault, "remote-source", server.instance + "/" + server.project + ".git", "--work-root", work,
                    "--username", USERNAME, "--ca-file", server.certificate, secrets=secrets)
            server.allow_fetch = False
            # Difference classes, including tag conflict and destination-only refs.
            source_head = git(server.repository, "rev-parse", "HEAD").decode().strip()
            git(server.repository, "update-ref", "refs/heads/main", "HEAD~1")
            git(server.repository, "update-ref", "refs/tags/v1", source_head)
            git(server.repository, "update-ref", "refs/heads/destination-only", source_head)
            git(server.repository, "update-ref", "-d", "refs/heads/feature")
            before = {p.relative_to(vault): p.read_bytes() for p in vault.rglob("*") if p.is_file()}

            def plan(mode="normal", *, token=TOKEN, ca=True, name="source", timeout=30, ok=True):
                server.mode = mode; server.repo_reads = 0; server.policy_pages.clear(); server.external_requests.clear(); server.rpc_bodies.clear()
                result = command(binary, "migration", "plan", vault, name, "--provider", server.provider,
                                 "--repository", server.project, "--instance", server.instance, "--work-root", work, "--timeout-seconds", timeout,
                                 "--username", USERNAME, *( ["--ca-file", server.certificate] if ca else []),
                                 secrets={**secrets, "token": token}, ok=ok)
                if not ok:
                    return result
                parsed = json.loads(result.stdout)
                assert parsed["summary"]["executionSupported"] is False
                assert parsed["summary"]["writeAuthorization"] == "unproven"
                assert not any("external.invalid" in path or "redirect-trap" in path for _, path in server.external_requests)
                assert all(method == "GET" or path.endswith("/git-upload-pack") for method, path in server.external_requests)
                return parsed

            for provider, prefix in [("github", ""), ("gitlab", "/platform")]:
                server.provider = provider; server.prefix = prefix
                good = plan()
                assert not good["summary"]["blockers"], good
                assert good["summary"]["repository"]["id"] == 21
                assert good["summary"]["actor"]["login"] == USERNAME
                changes = {row["name"]: row["change"] for row in good["rows"]}
                assert changes["refs/heads/main"] == "needs_ancestry_check"
                assert changes["refs/heads/feature"] == "create_candidate"
                assert changes["refs/heads/destination-only"] == "destination_only_preserve"
                assert changes["refs/tags/v1"] == "tag_conflict_preserve"
                assert changes["refs/tags/lightweight"] == "unchanged"
                assert good["summary"]["sourceExcludedRefs"] > 0
                assert good["summary"]["destinationFingerprint"]
                assert server.rpc_bodies, "No real Git protocol observation"
                paged = plan("pagination")
                assert all(p["complete"] for p in paged["summary"]["policies"])
                assert any(page == 2 for _, page in server.policy_pages)
                unavailable = plan("policy-denied")
                assert all(not p["complete"] for p in unavailable["summary"]["policies"])
                assert "policy_coverage_incomplete_never_assume_unprotected" in unavailable["summary"]["warnings"]
                for mode in ("missing", "denied", "rate", "redirect", "invalid-json", "oversized", "hostile-clone", "git-denied", "changed-identity"):
                    bad = plan(mode)
                    assert bad["summary"]["blockers"] and bad["rows"] == [], (mode, bad)
                assert plan("archived")["summary"]["blockers"] == ["destination_archived"]
                untrusted = plan(ca=False)
                assert untrusted["rows"] == [] and untrusted["summary"]["apiIssues"][0]["code"] == "https_request_failed"
                wrong = plan(token="wrong-token")
                assert wrong["rows"] == [] and wrong["summary"]["apiIssues"][0]["code"] == "authentication_rejected"
            timeout=plan("slow-api",timeout=1)
            assert timeout["rows"]==[] and timeout["summary"]["apiIssues"][0]["code"]=="timeout"
            remote = plan(name="remote-source")
            assert remote["summary"]["sourceVerification"] == "authenticated_manifest" and remote["rows"]
            server.provider="github"; server.prefix=""
            same=plan(name="remote-source")
            assert "source_and_destination_same_clone_url" in same["summary"]["blockers"]
            changed = plan("changed-refs")
            assert "destination_refs_changed_during_preflight" in changed["summary"]["blockers"] and not changed["rows"]
            assert before == {p.relative_to(vault): p.read_bytes() for p in vault.rglob("*") if p.is_file()}, "Read-only plan changed vault files"
            original = server.repository.with_name("original.git")
            server.repository.rename(original)
            empty = server.repository
            empty.mkdir()
            git(empty,"init","--bare","--initial-branch=main")
            empty_plan=plan()
            assert "empty_destination_object_format_unknown" in empty_plan["summary"]["warnings"]
            assert empty_plan["rows"] and all(row["change"]=="create_candidate" for row in empty_plan["rows"])
            empty.rename(empty.with_name("empty.git"))
            sha = fixture(root / "sha256-source", "sha256")
            git(sha,"clone","--mirror",str(sha),str(server.repository))
            incompatible=plan()
            assert "incompatible_git_object_format" in incompatible["summary"]["blockers"] and not incompatible["rows"]
            # Authentication must consume the tail, not trust a readable header.
            objects = list((vault / "data/objects").glob("*"))
            assert len(objects) == 2
            for path in objects:
                data = bytearray(path.read_bytes()); data[-1] ^= 1; path.write_bytes(data)
            plan(ok=False)
            assert server.external_requests == [], "Corrupted source contacted destination"
        encrypted_files(work)
        print("GitHub/GitLab TLS API identity/policy/ref preflight, no writes, no token leaks, source authentication and stale/partial-plan checks passed.")


if __name__ == "__main__":
    main()
