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
from test_remote import HTTPSFixture, TOKEN, USERNAME, command, probe_synthetic_git

class CreationFixture(HTTPSFixture):
    def __init__(self,root,provider="github",mode="normal"):
        self.provider=provider;self.mode=mode;self.exists=False
        self.project=USERNAME+"/new-private";self.prefix="/platform" if provider=="gitlab" else ""
        self.repo_id=91;self.description="";self.private=True;self.actions=True;self.protections={}
        self.api_calls=[];self.pushes=[];self.created_guard=None;self.race_oid=None
        super().__init__(root,lfs=True)
        self.lfs_store={};self.lfs_delay=0.025
        self.repository.rename(self.repository.with_name("source-mirror.git"))
        self.allow_receive=True
    @property
    def instance(self):return self.url.removesuffix("/project.git")+self.prefix
    @property
    def lfs_url(self):return self.instance+"/"+self.project+".git"
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
        expected=None;expected_lfs=None
        for provider in ("github","gitlab"):
            modes=["normal","bad-scope","public-response","hostile-clone","race-guard","verify-change","lost-create","existing","create-conflict","no-atomic","lfs-corrupt","lfs-missing","lfs-upload-error"]
            if provider=="github":modes.extend(["normal-main","normal-free"])
            for mode in modes:
                with CreationFixture(root/(provider+mode),provider,mode) as server:
                    if expected is None:
                        from benchmark_lfs import seed_profile
                        seed_profile(server,[64*1024]*8,update_mirror=False)
                        expected_lfs=dict(server.lfs_source_objects);server.lfs_store={}
                        command(binary,"git","capture",vault,"source",server.source,"--work-root",work,"--include-lfs")
                        expected={line.split()[1]:line.split()[0] for line in git(server.source,"for-each-ref","--format=%(objectname) %(refname)","refs/heads","refs/tags").decode().splitlines()}
                    if mode=="existing":server.create({"name":"new-private","description":"existing unrelated","private":True,"auto_init":True,"namespace_id":100,"visibility":"private","initialize_with_readme":True,"builds_access_level":"disabled"})
                    def call(verb,item,*extra,ok=True,timeout=30):
                        dest=["--provider",provider,"--repository",server.project,"--instance",server.instance] if verb=="plan-new" else ["--confirm-repository",server.project]
                        try:return command(binary,"migration",verb,vault,item,"--work-root",work,"--ca-file",server.certificate,"--timeout-seconds",timeout,*dest,*extra,*( ["--lfs-workers",4] if verb=="execute-new" else []),secrets=secrets,ok=ok)
                        except AssertionError:
                            if ok and verb=="execute-new" and server.exists:
                                print("SYNTHETIC_MIGRATION_REQUESTS="+json.dumps({"provider":provider,"mode":mode,"pushes":len(server.pushes),"lfsMethods":[method for method,_ in server.lfs_transfers]},separators=(",",":")),flush=True)
                                probe_synthetic_git(server,work)
                            raise
                    if provider=="github" and mode=="normal":
                        command(binary,"git","capture",vault,"git-only-lfs",server.source,"--work-root",work)
                        legacy=json.loads(call("plan-new","git-only-lfs").stdout)
                        call("execute-new",legacy["id"],ok=False)
                        assert not server.exists and not server.pushes and not any(method=="POST" for method,_,_ in server.api_calls),"Incomplete old LFS snapshot created a destination"
                    if mode in ("bad-scope","existing"):
                        before=inventory(server.repository) if server.exists else None
                        call("plan-new","source",ok=False)
                        assert not server.pushes and not any(method=="POST" for method,_,_ in server.api_calls)
                        if before is not None:assert inventory(server.repository)==before
                        continue
                    if mode.startswith("lfs-"):server.lfs_mode=mode.removeprefix("lfs-")
                    plan=json.loads(call("plan-new","source").stdout)
                    assert plan["lfsObjects"]==len(expected_lfs) and plan["lfsBytes"]>0
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
                        assert server.lfs_store==expected_lfs
                        server.wait_lfs_idle()
                        assert 2<=server.lfs_peak<=4
                        puts=[oid for method,oid in server.lfs_transfers if method=="PUT"]
                        assert len(puts)==len(set(puts))==len(expected_lfs)
                    saved=json.loads(command(binary,"migration","inspect",vault,plan["id"]).stdout)
                    assert saved["state"]==outcome["state"]
                    if mode in ("normal","lost-create"):
                        api_start=len(server.api_calls);pushes_before=len(server.pushes)
                        previous_reports=[]
                        def observe():
                            report=json.loads(command(binary,"migration","reconcile",vault,plan["id"],"--work-root",work,"--ca-file",server.certificate,secrets=secrets).stdout)
                            assert report["state"]==saved["state"],"Observation rewrote execution result"
                            reports=report["reconciliations"]
                            # Identify the newly saved immutable report by its unique ID.
                            fresh=[r for r in reports if r not in previous_reports]
                            previous_reports[:]=reports
                            assert len(fresh)==1,(fresh,reports)
                            return fresh[0]
                        report=observe()
                        if mode=="lost-create":
                            assert report["state"]=="unavailable" and report["issues"]==["no_durable_repository_id_do_not_adopt_destination"]
                            assert len(server.api_calls)==api_start,"Unknown creation identity triggered network adoption"
                        else:
                            assert report["state"]=="matches_recorded_refs",report
                            git(server.repository,"update-ref","refs/heads/concurrent-extra","HEAD")
                            report=observe()
                            assert report["state"]=="discrepancies" and any(row["name"]=="refs/heads/concurrent-extra" and row["expectedOid"] is None for row in report["differences"]),report
                            git(server.repository,"update-ref","-d","refs/heads/concurrent-extra")
                            server.private=False
                            assert observe()["state"]=="unavailable"
                            server.private=True
                            server.repo_id+=1
                            assert observe()["state"]=="unavailable"
                            server.repo_id-=1
                        assert len(server.pushes)==pushes_before,"Read-only report pushed Git refs"
                        assert all(method=="GET" for method,_,_ in server.api_calls[api_start:]),"Reconciliation mutated provider API"
                    if mode=="race-guard":assert git(server.repository,"rev-parse","HEAD").decode().strip()==server.race_oid
                    if mode in ("public-response","hostile-clone","lost-create","create-conflict","lfs-corrupt","lfs-missing","lfs-upload-error"):assert not server.pushes
                    if mode=="lfs-upload-error":
                        server.wait_lfs_idle();assert len(server.lfs_transfers)<=4
                        assert not server.lfs_store and not server.pushes
                    assert not any(method=="DELETE" and "protected_branches/" not in path for method,path,_ in server.api_calls)
        backup=root/"receipts.rsbackup";command(binary,"vault","backup",vault,backup)
        recovered=root/"recovered";command(binary,"vault","restore",backup,recovered)
        assert command(binary,"vault","list",recovered).stdout==command(binary,"vault","list",vault).stdout
        history=json.loads(command(binary,"migration","history",recovered,"--limit",20).stdout)
        assert history["total"]>=20 and len(history["items"])==20
        assert json.loads(command(binary,"migration","inspect",recovered,plan["id"]).stdout)==json.loads(command(binary,"migration","inspect",vault,plan["id"]).stdout),"Recovered receipt details changed"
        for p in vault.rglob("*"):
            if p.is_file():assert TOKEN.encode() not in p.read_bytes()
        encrypted_files(work)
        print("Guarded GitHub/GitLab private creation, atomic leases, existing protection, no-retry receipts, independent history verification and encrypted report recovery passed.")

if __name__=="__main__":main()
