"""Guarded creation and real smart-HTTP receive-pack, synthetic personal hosts."""
import argparse
import io
import json
import os
import pathlib
import tempfile
import time
import urllib.parse
from test_git import PASSWORD, git, encrypted_files, inventory
from test_remote import HTTPSFixture, TOKEN, USERNAME, command

class CreationFixture(HTTPSFixture):
    def __init__(self,root,provider="github",mode="normal"):
        self.provider=provider;self.mode=mode;self.exists=False
        self.project=USERNAME+"/new-private";self.prefix="/platform" if provider=="gitlab" else ""
        self.repo_id=91;self.description="";self.private=True;self.actions=True;self.protections={}
        self.api_calls=[];self.pushes=[];self.created_guard=None;self.race_oid=None
        super().__init__(root)
        self.repository.rename(self.repository.with_name("source-mirror.git"))
        self.allow_receive=True
    @property
    def instance(self):return self.url.removesuffix("/project.git")+self.prefix
    def metadata(self):
        value={"id":self.repo_id,"archived":False,"default_branch":self.default_branch,"description":self.description}
        clone=self.instance+"/"+self.project+".git"
        if self.mode=="hostile-clone":clone="https://external.invalid/fixture/new-private.git"
        if self.provider=="github":value.update(full_name=self.project,clone_url=clone,disabled=False,private=self.private,owner={"id":10},permissions={"push":True})
        else:value.update(path_with_namespace=self.project,http_url_to_repo=clone,visibility="private" if self.private else "public",namespace={"id":100,"kind":"user","full_path":USERNAME},permissions={"project_access":{"access_level":50},"group_access":None},builds_access_level="disabled")
        return value
    def create(self,body):
        assert not self.exists
        assert body["name"]==self.project.split('/')[1]
        if self.provider=="github":assert body["private"] is True and body["auto_init"] is True
        else:
            assert body["visibility"]=="private" and body["initialize_with_readme"] is True and body["builds_access_level"]=="disabled" and body["namespace_id"]==100
            self.actions=False
        self.description=body["description"];self.private=self.mode!="public-response"
        self.default_branch=body.get("default_branch","main" if self.mode=="normal-main" else "bootstrap")
        self.created_guard=self.default_branch
        source=self.root/"guard-initialization";source.mkdir()
        git(source,"init","--initial-branch="+self.default_branch)
        (source/"README.md").write_text("New repository creation guard\n"+self.description)
        git(source,"add",".");git(source,"commit","-m","initialized new private repository")
        git(source,"clone","--bare",str(source),str(self.repository));git(self.repository,"config","http.receivepack","true")
        self.exists=True
        if self.mode=="no-atomic":git(self.repository,"config","receive.advertiseAtomic","false")
        if self.provider=="gitlab":self.protections[self.default_branch]=self.policy(self.default_branch)
    def policy(self,name):return {"name":name,"allow_force_push":False,"push_access_levels":[{"access_level":40}],"merge_access_levels":[{"access_level":40}]}
    def handler(self):
        parent=super().handler();owner=self
        class Handler(parent):
            def do_PUT(self):self.process()
            def do_PATCH(self):self.process()
            def do_DELETE(self):self.process()
            def process(self):
                parsed=urllib.parse.urlsplit(self.path);path=parsed.path
                api=owner.prefix+("/api/v3/" if owner.provider=="github" else "/api/v4/")
                if path.startswith(api):
                    assert self.headers.get("Authorization")=="Bearer "+TOKEN
                    length=int(self.headers.get("Content-Length","0"));assert length<16384
                    body=json.loads(self.rfile.read(length)) if length else None
                    owner.api_calls.append((self.command,path,body))
                    def send(v,code=200,headers=None):return self.send(code,json.dumps(v).encode() if code!=204 else b"",headers)
                    if path==api+"user":return send({"id":10,"login" if owner.provider=="github" else "username":USERNAME},headers={"X-OAuth-Scopes":"repo, workflow" if owner.mode!="bad-scope" else "read:user"})
                    if path==api+"personal_access_tokens/self":return send({"active":True,"revoked":False,"scopes":["api","write_repository"] if owner.mode!="bad-scope" else ["read_api"]})
                    if path==api+"namespaces/"+USERNAME:return send({"id":100,"kind":"user","full_path":USERNAME})
                    if path==api+("user/repos" if owner.provider=="github" else "projects") and self.command=="POST":
                        if owner.exists:return send({"message":TOKEN},422)
                        owner.create(body)
                        if owner.mode=="create-conflict":return send({"message":"exists"},422)
                        if owner.mode=="lost-create":time.sleep(3)
                        return send(owner.metadata(),201)
                    repo=api+("repos/"+owner.project if owner.provider=="github" else "projects/"+urllib.parse.quote(owner.project,safe=""))
                    if path==repo:
                        if not owner.exists:return send({},404)
                        if self.command=="GET":return send(owner.metadata())
                        assert self.command in ("PATCH","PUT") and set(body)=={"default_branch"}
                        assert not owner.actions
                        owner.default_branch=body["default_branch"];git(owner.repository,"symbolic-ref","HEAD","refs/heads/"+owner.default_branch)
                        return send(owner.metadata())
                    if path==repo+"/actions/permissions":
                        if self.command=="PUT":assert body=={"enabled":False};owner.actions=False;return send(None,204)
                        return send({"enabled":owner.actions})
                    branch_prefix=repo+("/branches/" if owner.provider=="github" else "/repository/branches/")
                    if path.startswith(branch_prefix):
                        branch=urllib.parse.unquote(path[len(branch_prefix):]);oid=git(owner.repository,"rev-parse","refs/heads/"+branch).decode().strip()
                        return send({"commit":{"sha" if owner.provider=="github" else "id":oid}})
                    if path in (repo+"/branches",repo+"/rulesets") and self.command=="GET":
                        if path.endswith("/rulesets") and owner.mode=="normal-free":return send({"message":"Upgrade to GitHub Pro or make this repository public to enable this feature.","documentation_url":"https://docs.github.com/rest/repos/rules#get-all-repository-rulesets"},403)
                        return send([])
                    protection=api+f"projects/{owner.repo_id}/protected_branches"
                    if path==protection and self.command=="GET":return send(list(owner.protections.values()))
                    if path==protection and self.command=="POST":
                        assert set(body)=={"name","push_access_level","merge_access_level","allow_force_push"}
                        assert body["allow_force_push"] is False and body["push_access_level"]==40 and body["merge_access_level"]==40
                        p=owner.policy(body["name"]);owner.protections[body["name"]]=p;return send(p,201)
                    if path.startswith(protection+"/"):
                        name=urllib.parse.unquote(path[len(protection)+1:])
                        if name not in owner.protections:return send({},404)
                        if self.command=="DELETE":
                            assert name==owner.created_guard and name!=owner.default_branch
                            del owner.protections[name];return send(None,204)
                        return send(owner.protections[name])
                    return send({},404)
                remote=owner.prefix+"/"+owner.project+".git"
                if path.startswith(remote+"/"):
                    if path.endswith("git-receive-pack") and self.command=="POST":
                        length=int(self.headers.get("Content-Length","0"));data=self.rfile.read(length)
                        # Source automation must be off before any object upload.
                        assert not owner.actions or owner.provider=="gitlab"
                        assert b"atomic" in data[:2048]
                        owner.pushes.append(data);self.rfile=io.BytesIO(data)
                        if owner.mode=="race-guard" and len(owner.pushes)==1:
                            old=git(owner.repository,"rev-parse","HEAD").decode().strip();tree=git(owner.repository,"rev-parse","HEAD^{tree}").decode().strip()
                            owner.race_oid=git(owner.repository,"commit-tree",tree,"-p",old,"-m","concurrent edit").decode().strip()
                            git(owner.repository,"update-ref","refs/heads/"+owner.created_guard,owner.race_oid)
                    self.path=self.path.replace(remote,"/project.git",1)
                    result=super().process()
                    if owner.mode=="verify-change" and path.endswith("git-receive-pack") and len(owner.pushes)==2:
                        git(owner.repository,"update-ref","refs/heads/concurrent-extra","HEAD")
                    return result
                return self.send(404)
        return Handler


def main():
    parser=argparse.ArgumentParser();parser.add_argument("binary",type=pathlib.Path);args=parser.parse_args();binary=args.binary.resolve()
    parent=pathlib.Path(os.environ.get("RS_ENCRYPTED_PARENT",tempfile.gettempdir()))
    with tempfile.TemporaryDirectory(prefix="rs-new-") as tmp,tempfile.TemporaryDirectory(prefix="rs-new-encrypted-",dir=parent) as encrypted:
        root=pathlib.Path(tmp);work=pathlib.Path(encrypted)/"work";vault=root/"vault"
        command(binary,"workspace","init",work);command(binary,"vault","init",vault)
        secrets={"vault_passphrase":PASSWORD.decode().strip(),"token":TOKEN}
        expected=None
        for provider in ("github","gitlab"):
            modes=["normal","bad-scope","public-response","hostile-clone","race-guard","verify-change","lost-create","existing","create-conflict","no-atomic"]
            if provider=="github":modes.extend(["normal-main","normal-free"])
            for mode in modes:
                with CreationFixture(root/(provider+mode),provider,mode) as server:
                    if expected is None:
                        command(binary,"git","capture",vault,"source",server.source,"--work-root",work)
                        expected={line.split()[1]:line.split()[0] for line in git(server.source,"for-each-ref","--format=%(objectname) %(refname)","refs/heads","refs/tags").decode().splitlines()}
                    if mode=="existing":server.create({"name":"new-private","description":"existing unrelated","private":True,"auto_init":True,"namespace_id":100,"visibility":"private","initialize_with_readme":True,"builds_access_level":"disabled"})
                    def call(verb,item,*extra,ok=True,timeout=30):
                        dest=["--provider",provider,"--repository",server.project,"--instance",server.instance] if verb=="plan-new" else ["--confirm-repository",server.project]
                        return command(binary,"migration",verb,vault,item,"--work-root",work,"--ca-file",server.certificate,"--timeout-seconds",timeout,*dest,*extra,secrets=secrets,ok=ok)
                    if mode in ("bad-scope","existing"):
                        before=inventory(server.repository) if server.exists else None
                        call("plan-new","source",ok=False)
                        assert not server.pushes and not any(method=="POST" for method,_,_ in server.api_calls)
                        if before is not None:assert inventory(server.repository)==before
                        continue
                    plan=json.loads(call("plan-new","source").stdout)
                    assert plan["visibility"]=="private" and plan["automation"]=="disabled"
                    outcome=json.loads(call("execute-new",plan["id"],ok=mode in ("normal","normal-main","normal-free"),timeout=1 if mode=="lost-create" else 30).stdout)
                    assert outcome["state"]==("verified" if mode in ("normal","normal-main","normal-free") else "needs_attention"),outcome
                    posts=sum(method=="POST" and path.endswith(("user/repos","projects")) for method,path,_ in server.api_calls)
                    assert posts==1
                    call("execute-new",plan["id"],ok=False)
                    assert sum(method=="POST" and path.endswith(("user/repos","projects")) for method,path,_ in server.api_calls)==1,"Consumed plan retried creation"
                    if mode in ("normal","normal-main","normal-free"):
                        actual={line.split()[1]:line.split()[0] for line in git(server.repository,"for-each-ref","--format=%(objectname) %(refname)").decode().splitlines()}
                        assert actual==expected
                        assert git(server.repository,"symbolic-ref","HEAD").strip()==b"refs/heads/main"
                        assert len(server.pushes)==2
                    if mode=="race-guard":assert git(server.repository,"rev-parse","HEAD").decode().strip()==server.race_oid
                    if mode in ("public-response","hostile-clone","lost-create","create-conflict"):assert not server.pushes
                    assert not any(method=="DELETE" and "protected_branches/" not in path for method,path,_ in server.api_calls)
        backup=root/"receipts.rsbackup";command(binary,"vault","backup",vault,backup)
        recovered=root/"recovered";command(binary,"vault","restore",backup,recovered)
        assert command(binary,"vault","list",recovered).stdout==command(binary,"vault","list",vault).stdout
        for p in vault.rglob("*"):
            if p.is_file():assert TOKEN.encode() not in p.read_bytes()
        encrypted_files(work)
        print("Guarded GitHub/GitLab private creation, atomic leases, existing protection, no-retry receipts, independent history verification and encrypted report recovery passed.")

if __name__=="__main__":main()
