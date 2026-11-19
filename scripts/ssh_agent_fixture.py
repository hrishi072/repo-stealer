"""Synthetic identities only; Windows service changes require a hosted CI runner."""
import contextlib
import json
import os
import pathlib
import re
import subprocess
import uuid


class SSHAgent:
    def __init__(self):
        self.original_service = None
        self.tool_env = None
        self.baseline = None
        self.saved_env = {k: os.environ.get(k) for k in ("SSH_AUTH_SOCK", "SSH_AGENT_PID")}

    @staticmethod
    def powershell(script):
        result = subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
             "$ErrorActionPreference='Stop'; " + script],
            capture_output=True, text=True, timeout=40, check=True,
        )
        return result.stdout.strip()

    def service_status(self):
        return json.loads(self.powershell(
            "$s=Get-Service -Name ssh-agent; "
            "@{status=$s.Status.ToString();startType=$s.StartType.ToString()} | ConvertTo-Json -Compress"
        ))

    def service(self, running):
        action, state = ("Start", "Running") if running else ("Stop", "Stopped")
        self.powershell(
            f"{action}-Service -Name ssh-agent; "
            f"(Get-Service -Name ssh-agent).WaitForStatus('{state}',[TimeSpan]::FromSeconds(15))"
        )

    def __enter__(self):
        try:
            if os.name == "nt":
                assert (os.environ.get("GITHUB_ACTIONS") == "true"
                        and os.environ.get("RUNNER_OS") == "Windows"
                        and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted"), (
                    "Windows agent service tests run only on disposable GitHub-hosted Windows runners"
                )
                self.tool = str(pathlib.Path(os.environ["SystemRoot"]) / "System32/OpenSSH/ssh-add.exe")
                self.original_service = self.service_status()
                assert self.original_service["status"] in ("Running", "Stopped")
                if self.original_service["startType"] == "Disabled":
                    self.powershell("Set-Service -Name ssh-agent -StartupType Manual")
                self.service(True)
                self.tool_env = dict(os.environ, SSH_AUTH_SOCK=r"\\.\pipe\openssh-ssh-agent")
                # App children must use the documented native Windows service,
                # even when launched from a terminal with an unrelated socket.
                os.environ["SSH_AUTH_SOCK"] = r"\\.\pipe\rs-missing-" + uuid.uuid4().hex
                os.environ.pop("SSH_AGENT_PID", None)
            else:
                self.tool = "ssh-add"
                output = subprocess.check_output(["ssh-agent", "-s"], text=True, timeout=15)
                socket = re.search(r"SSH_AUTH_SOCK=([^;]+)", output).group(1)
                pid = re.search(r"SSH_AGENT_PID=([0-9]+)", output).group(1)
                self.tool_env = dict(os.environ, SSH_AUTH_SOCK=socket, SSH_AGENT_PID=pid)
                os.environ.update(SSH_AUTH_SOCK=socket, SSH_AGENT_PID=pid)
            self.baseline = self.keys()
            return self
        except BaseException:
            self.__exit__(None, None, None)
            raise

    def run(self, *args, input=None, check=True):
        return subprocess.run([self.tool, *args], input=input, capture_output=True,
                              env=self.tool_env, timeout=15, check=check)

    def keys(self):
        result = self.run("-L", check=False)
        assert result.returncode in (0, 1), "Cannot query the fixture SSH agent"
        if result.returncode == 1:
            return set()
        return {tuple(line.split()[:2]) for line in result.stdout.splitlines()}

    @contextlib.contextmanager
    def identity(self, server):
        before = self.keys()
        public = f"{server.user_key.get_name()} {server.user_key.get_base64()}\n".encode()
        identity = tuple(public.split()[:2])
        assert identity not in before
        hidden = server.private_key.with_name("unavailable-fixture-key")
        try:
            # ssh-add's stdin mode avoids copying synthetic key bytes into a
            # second file or changing the source key's Windows ACL.
            self.run("-", input=server.private_key.read_bytes())
            assert self.keys() == before | {identity}
            server.private_key.rename(hidden)
            yield
        finally:
            if hidden.exists():
                hidden.rename(server.private_key)
            if identity in self.keys():
                self.run("-d", "-", input=public)
            assert self.keys() == before, "Fixture changed unrelated agent identities"

    @contextlib.contextmanager
    def unavailable(self):
        assert os.name == "nt" and self.original_service is not None
        before = self.keys()
        try:
            self.service(False)
            yield
        finally:
            self.service(True)
            assert self.keys() == before

    def __exit__(self, *_):
        try:
            if self.baseline is not None:
                assert self.keys() == self.baseline, "Fixture agent identities were not restored"
        finally:
            try:
                if os.name == "nt" and self.original_service is not None:
                    self.service(self.original_service["status"] == "Running")
                    if self.original_service["startType"] == "Disabled":
                        self.powershell("Set-Service -Name ssh-agent -StartupType Disabled")
                    assert self.service_status() == self.original_service
                elif os.name != "nt" and self.tool_env is not None:
                    subprocess.run(["ssh-agent", "-k"], env=self.tool_env,
                                   capture_output=True, timeout=15, check=True)
            finally:
                for key, value in self.saved_env.items():
                    if value is None:
                        os.environ.pop(key, None)
                    else:
                        os.environ[key] = value
