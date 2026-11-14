"""Real OpenSSH/Git against a loopback-only synthetic SSH server; no user keys."""
import argparse
import base64
import contextlib
import hashlib
import logging
import os
import pathlib
import shlex
import socket
import subprocess
import tempfile
import threading
import time

import paramiko
from test_git import PASSWORD, app, encrypted_files, fixture, git, inventory

logging.getLogger("paramiko").setLevel(logging.CRITICAL)


class SSHFixture:
    def __init__(self, root):
        self.root=pathlib.Path(root);self.root.mkdir()
        self.source=fixture(self.root/"source")
        self.repository=self.root/"project.git"
        git(self.root,"clone","--mirror",str(self.source),str(self.repository))
        self.host_key=paramiko.RSAKey.generate(2048)
        self.user_key=paramiko.RSAKey.generate(2048)
        self.private_key=self.root/"synthetic key's identity.pem"
        self.user_key.write_private_key_file(str(self.private_key));self.private_key.chmod(0o600)
        self.host_public=self.root/"host.pub"
        self.host_public.write_text(f"{self.host_key.get_name()} {self.host_key.get_base64()}\n")
        self.fingerprint="SHA256:"+base64.b64encode(hashlib.sha256(self.host_key.asbytes()).digest()).decode().rstrip("=")
        self.commands=[];self.auth_attempts=[];self.mode="normal";self.active=[];self.children=[];self.stop=threading.Event();self.executing=threading.Event()
        self.listener=socket.socket();self.listener.bind(("127.0.0.1",0));self.listener.listen();self.listener.settimeout(.2)
        self.port=self.listener.getsockname()[1];self.url=f"ssh://git@127.0.0.1:{self.port}/project.git"
        self.thread=threading.Thread(target=self.accept,daemon=True)

    def __enter__(self):self.thread.start();return self
    def __exit__(self,*_):
        self.stop.set();self.listener.close()
        for transport in self.active:transport.close()
        for child in self.children:
            if child.poll() is None:child.kill()
            child.wait(timeout=10)
        self.thread.join(timeout=5)

    def accept(self):
        while not self.stop.is_set():
            try:peer,_=self.listener.accept()
            except (TimeoutError,socket.timeout):continue
            except OSError:return
            threading.Thread(target=self.connection,args=(peer,),daemon=True).start()

    def connection(self,peer):
        fixture=self;event=threading.Event();requested=[]
        class Server(paramiko.ServerInterface):
            def get_allowed_auths(self,username):return "publickey"
            def check_auth_publickey(self,username,key):
                fixture.auth_attempts.append(username)
                return paramiko.AUTH_SUCCESSFUL if username=="git" and key==fixture.user_key else paramiko.AUTH_FAILED
            def check_channel_request(self,kind,chanid):
                return paramiko.OPEN_SUCCEEDED if kind=="session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
            def check_channel_exec_request(self,channel,command):
                fields=shlex.split(command.decode())
                if len(fields)!=2 or fields[0]!="git-upload-pack" or fields[1] not in ("/project.git","project.git"):return False
                requested.append(command);event.set();return True
        transport=paramiko.Transport(peer);self.active.append(transport)
        try:
            transport.add_server_key(self.host_key);transport.start_server(server=Server())
            channel=transport.accept(15)
            if not channel or not event.wait(15):return
            self.commands.extend(requested);self.executing.set()
            if self.mode=="stall":
                while transport.is_active() and not self.stop.wait(.05):pass
                return
            if self.mode=="mutate" and len(self.commands)>=3:
                git(self.repository,"update-ref","refs/heads/feature",git(self.repository,"rev-parse","HEAD").decode().strip())
            child=subprocess.Popen(["git","-c","core.hooksPath="+str(self.root/"empty-hooks"),"upload-pack",str(self.repository)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env={k:v for k,v in os.environ.items() if not k.startswith("GIT_")})
            self.children.append(child)
            def feed():
                try:
                    while data:=channel.recv(32768):child.stdin.write(data);child.stdin.flush()
                except (OSError,EOFError):pass
                finally:
                    with contextlib.suppress(OSError):child.stdin.close()
            threading.Thread(target=feed,daemon=True).start()
            try:
                while data:=os.read(child.stdout.fileno(),32768):channel.sendall(data)
                code=child.wait(timeout=20);channel.send_exit_status(code);channel.shutdown_write()
                deadline=time.monotonic()+3
                while transport.is_active() and not channel.closed and time.monotonic()<deadline:time.sleep(.01)
            except (OSError,EOFError):
                if child.poll() is None:child.kill()
            finally:child.stdout.close()
        except (paramiko.SSHException,EOFError,OSError):pass
        finally:transport.close();peer.close()


def probe_windows_client(server,work):
    """Diagnostics on synthetic fixture data only; never enable app secret logs."""
    client=pathlib.Path(os.environ["SystemRoot"])/"System32/OpenSSH/ssh.exe"
    print("Synthetic SSH native client version:",subprocess.run([str(client),"-V"],capture_output=True,text=True).stderr,flush=True)
    probe=work/"client-probe";probe.mkdir()
    identity=probe/"identity.pem";identity.write_bytes(server.private_key.read_bytes());identity.chmod(0o600)
    hosts=probe/"known_hosts";hosts.write_text(f"repo-stealer-source ssh-rsa {server.host_key.get_base64()}\n")
    config=probe/"config"
    def quoted(path):return '"'+path.as_posix().replace('"','\\"')+'"'
    config.write_text("Host *\n BatchMode yes\n StrictHostKeyChecking yes\n HostKeyAlias repo-stealer-source\n GlobalKnownHostsFile none\n IdentityAgent none\n IdentitiesOnly yes\n HostKeyAlgorithms rsa-sha2-512,rsa-sha2-256\n UserKnownHostsFile "+quoted(hosts)+"\n IdentityFile "+quoted(identity)+"\n")
    command=[str(client),"-vvv","-F",str(config),"-p",str(server.port),"git@127.0.0.1","git-upload-pack '/project.git'"]
    result=subprocess.run(command,input=b"",capture_output=True,timeout=20)
    print("Synthetic direct SSH result:",result.returncode,result.stderr.decode(errors="replace")[-16000:],flush=True)
    print("Synthetic fixture counts:",len(server.auth_attempts),len(server.commands),flush=True)


def main():
    parser=argparse.ArgumentParser();parser.add_argument("binary",type=pathlib.Path);args=parser.parse_args();binary=args.binary.resolve()
    parent=os.environ.get("RS_ENCRYPTED_PARENT",tempfile.gettempdir())
    with tempfile.TemporaryDirectory(prefix="rs-ssh-") as temp,tempfile.TemporaryDirectory(prefix="rs-ssh-encrypted-",dir=parent) as encrypted:
        root=pathlib.Path(temp);work=pathlib.Path(encrypted)/"work with spaces and 'quote";app(binary,"workspace","init",work)
        vault=root/"vault";app(binary,"vault","init",vault)
        with SSHFixture(root/"server") as server:
            def capture(name,ok=True,key=None,host=None,extra=()):
                try:
                    return app(binary,"git","capture-ssh",vault,name,server.url,"--work-root",work,"--ssh-key",key or server.private_key,"--ssh-host-key",host or server.host_public,*extra,ok=ok)
                except AssertionError:
                    if ok and os.name=="nt":probe_windows_client(server,work)
                    raise
            # Wrong host identity must fail before offering any user key.
            other=paramiko.RSAKey.generate(2048);wrong=root/"wrong.pub";wrong.write_text(f"ssh-rsa {other.get_base64()}\n")
            capture("wrong-host",ok=False,host=wrong);assert not server.auth_attempts
            bad=root/"bad.pem";other.write_private_key_file(str(bad));bad.chmod(0o600)
            capture("bad-auth",ok=False,key=bad);assert not server.commands
            capture("lfs-refused",ok=False,extra=("--include-lfs",));assert not server.commands
            capture("ssh-history")
            restored=work/"restored.git";app(binary,"git","restore",vault,"ssh-history",restored,"--work-root",work)
            assert inventory(server.repository)==inventory(restored)
            if os.name!="nt":
                # Use an isolated fixture agent; never touch the developer's agent.
                import re
                output=subprocess.check_output(["ssh-agent","-s"],text=True)
                socket_path=re.search(r"SSH_AUTH_SOCK=([^;]+)",output).group(1)
                pid=re.search(r"SSH_AGENT_PID=([0-9]+)",output).group(1)
                env=dict(os.environ,SSH_AUTH_SOCK=socket_path,SSH_AGENT_PID=pid)
                try:
                    subprocess.run(["ssh-add",str(server.private_key)],env=env,capture_output=True,check=True)
                    command=[str(binary),"git","capture-ssh",str(vault),"agent-history",server.url,"--work-root",str(work),"--ssh-host-key",str(server.host_public),"--password-stdin"]
                    result=subprocess.run(command,input=PASSWORD,capture_output=True,env=env,timeout=60)
                    assert result.returncode==0,result.stderr
                finally:subprocess.run(["ssh-agent","-k"],env=env,capture_output=True,check=True)
            # No archive replacement on duplicate capture.
            count=len(server.commands);capture("ssh-history",ok=False);assert len(server.commands)==count
            server.commands.clear();server.mode="mutate";capture("changed",ok=False)
            server.mode="stall";server.executing.clear()
            command=[str(binary),"git","capture-ssh",str(vault),"timeout",server.url,"--work-root",str(work),"--ssh-key",str(server.private_key),"--ssh-host-key",str(server.host_public),"--password-stdin","--timeout-seconds","1"]
            started=time.monotonic();result=subprocess.run(command,input=PASSWORD,capture_output=True,timeout=15)
            assert result.returncode and time.monotonic()-started<10
            assert not list(work.glob(".rs-git-*")),"SSH staging was not removed after timeout"
            encrypted_files(work)
        print("Real SSH key authentication, strict host pinning before authentication, full-history offline restore, no-overwrite, source mutation, unsupported LFS refusal and timeout cleanup passed.")

if __name__=="__main__":main()
