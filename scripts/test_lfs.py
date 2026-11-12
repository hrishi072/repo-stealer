"""Full-history encrypted LFS capture, offline recovery and hostile TLS API cases."""
import argparse
import hashlib
import json
import os
import pathlib
import struct
import tempfile
from test_remote import HTTPSFixture, USERNAME, TOKEN, command
from test_git import PASSWORD, encrypted_files, inventory, git


def local_history(source):
    objects={}
    for index in (1,2):
        data=(b"RS_GIT_CONTENT_SENTINEL_01_LFS_"+str(index).encode())*4096
        oid=hashlib.sha256(data).hexdigest();objects[oid]=data
        (source/"lfs.bin").write_bytes((f"version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {len(data)}\n").encode("ascii"))
        path=source/".git/lfs/objects"/oid[:2]/oid[2:4]/oid;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
        git(source,"add","lfs.bin");git(source,"commit","-m",f"LFS history {index}")
        assert b"\r" not in git(source,"show","HEAD:lfs.bin"),"Fixture pointer must use canonical LF bytes"
    return objects


def check_restore(binary,vault,name,dest,work,expected=None):
    command(binary,"git","restore",vault,name,dest,"--work-root",work)
    files=[p for p in (dest/"lfs/objects").rglob("*") if p.is_file()]
    assert len(files)==2
    actual={p.name:p.read_bytes() for p in files}
    for oid,data in actual.items():assert hashlib.sha256(data).hexdigest()==oid
    if expected is not None:assert actual==expected
    assert not git(dest,"show","HEAD:large.bin").startswith(b"current encrypted"),"Restore smudged Git pointers"
    encrypted_files(work)


def main():
    p=argparse.ArgumentParser();p.add_argument("binary",type=pathlib.Path);p.add_argument("--fixture-out",type=pathlib.Path);p.add_argument("--restore-fixture",type=pathlib.Path);args=p.parse_args();binary=args.binary.resolve()
    parent=pathlib.Path(os.environ.get("RS_ENCRYPTED_PARENT",tempfile.gettempdir()))
    with tempfile.TemporaryDirectory(prefix="rs-lfs-") as tmp,tempfile.TemporaryDirectory(prefix="rs-lfs-encrypted-",dir=parent) as enc:
        root=pathlib.Path(tmp);work=pathlib.Path(enc)/"work";vault=root/"vault"
        command(binary,"workspace","init",work)
        if args.restore_fixture:
            command(binary,"vault","restore",args.restore_fixture.resolve(),vault)
            check_restore(binary,vault,"lfs-history",work/"cross-lfs.git",work)
            print("Cross-OS encrypted LFS payload recovery passed.");return
        command(binary,"vault","init",vault)
        with HTTPSFixture(root/"https",lfs=True) as server:
            secrets={"vault_passphrase":PASSWORD.decode().strip(),"token":TOKEN}
            def capture(name,ok=True,ca=True):
                return command(binary,"git","capture-remote",vault,name,server.url,"--include-lfs","--username",USERNAME,"--work-root",work,*( ["--ca-file",server.certificate] if ca else []),secrets=secrets,ok=ok)
            capture("untrusted-lfs",ok=False,ca=False)
            result=capture("lfs-history");report=json.loads(result.stdout.splitlines()[0]);assert report["lfs"]["objects"]==2
            assert report["lfs"]["bytes"]==sum(map(len,server.lfs_source_objects.values()))
            check_restore(binary,vault,"lfs-history",work/"remote-lfs.git",work,server.lfs_source_objects)
            assert inventory(server.repository)==inventory(work/"remote-lfs.git")
            for mode in ("corrupt","extra","missing","unsafe-url","redirect"):
                server.lfs_mode=mode;capture("reject-"+mode,ok=False)
            server.lfs_mode="normal"
            command(binary,"git","capture",vault,"local-lfs",server.source,"--include-lfs","--work-root",work)
            check_restore(binary,vault,"local-lfs",work/"local-lfs.git",work,server.lfs_source_objects)
            # Authenticated, malformed v3 payloads must never publish a destination.
            archive=root/"archive";command(binary,"vault","export",vault,"local-lfs",archive,"--plaintext")
            data=bytearray(archive.read_bytes());length=struct.unpack("<I",data[8:12])[0];meta=json.loads(data[12:12+length]);assert meta["version"]==3 and len(meta["lfs"]["objects"])==2
            data[-1]^=1;bad=root/"bad";bad.write_bytes(data)
            command(binary,"vault","put",vault,"bad-lfs",bad)
            dest=work/"must-not-exist.git";command(binary,"git","restore",vault,"bad-lfs",dest,"--work-root",work,ok=False);assert not dest.exists()
            oid=next(iter(server.lfs_source_objects));path=server.source/".git/lfs/objects"/oid[:2]/oid[2:4]/oid
            path.write_bytes(b"corrupt")
            command(binary,"git","capture",vault,"reject-local-corrupt",server.source,"--include-lfs","--work-root",work,ok=False)
            path.unlink()
            command(binary,"git","capture",vault,"reject-local-missing",server.source,"--include-lfs","--work-root",work,ok=False)
            listing=command(binary,"vault","list",vault).stdout;assert b"reject-" not in listing
            for file in vault.rglob("*"):
                if file.is_file() and file.name!="lock":
                    content=file.read_bytes();assert TOKEN.encode() not in content
                    for payload in server.lfs_source_objects.values():assert payload[:64] not in content
        # No server is alive during this recovery drill.
        backup=root/"lfs.rsbackup";command(binary,"vault","backup",vault,backup)
        recovered=root/"recovered";command(binary,"vault","restore",backup,recovered)
        check_restore(binary,recovered,"lfs-history",work/"offline-lfs.git",work,server.lfs_source_objects)
        if args.fixture_out:
            output=args.fixture_out.resolve();output.parent.mkdir(parents=True,exist_ok=True);command(binary,"vault","backup",vault,output)
        print("Historical LFS HTTPS/local capture, integrity failures, offline same-key recovery and encrypted workspace checks passed.")
if __name__=="__main__":main()
