"""Synthetic repeatable LFS measurements on real verified encrypted storage."""
import argparse
import hashlib
import json
import os
import pathlib
import platform
import statistics
import tempfile
import time
from test_remote import HTTPSFixture, USERNAME, TOKEN, command
from test_git import PASSWORD, encrypted_files, git


def seed_profile(server,sizes,update_mirror=True):
    """All payload bytes and committed pointer encodings are deterministic."""
    for index,size in enumerate(sizes):
        data=hashlib.shake_256(f"synthetic LFS profile {index} {size}".encode()).digest(size)
        oid=hashlib.sha256(data).hexdigest()
        server.lfs_source_objects[oid]=data;server.lfs_store[oid]=data
        (server.source/f"profile-{index}.bin").write_bytes(f"version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n".encode("ascii"))
        path=server.source/".git/lfs/objects"/oid[:2]/oid[2:4]/oid;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
    git(server.source,"add",".");git(server.source,"commit","-m","synthetic bounded transfer profile")
    if update_mirror:git(server.source,"push","--mirror",str(server.repository))


def main():
    parser=argparse.ArgumentParser();parser.add_argument("binary",type=pathlib.Path);parser.add_argument("--output",type=pathlib.Path,required=True);args=parser.parse_args();binary=args.binary.resolve()
    parent=pathlib.Path(os.environ.get("RS_ENCRYPTED_PARENT",tempfile.gettempdir()))
    samples=[]
    with tempfile.TemporaryDirectory(prefix="rs-lfs-benchmark-") as temporary,tempfile.TemporaryDirectory(prefix="rs-lfs-benchmark-encrypted-",dir=parent) as encrypted:
        root=pathlib.Path(temporary);work=pathlib.Path(encrypted)/"work";vault=root/"vault"
        command(binary,"workspace","init",work);command(binary,"vault","init",vault)
        profiles={"many-small":[64*1024]*16,"few-large":[1024*1024]*8,"mixed":[32*1024]*8+[256*1024]*4+[1024*1024]*2}
        secrets={"vault_passphrase":PASSWORD.decode().strip(),"token":TOKEN}
        for profile,sizes in profiles.items():
            with HTTPSFixture(root/profile,lfs=True) as server:
                seed_profile(server,sizes);server.lfs_delay=0.1
                total=sum(map(len,server.lfs_source_objects.values()))
                for repeat in range(3):
                    # Alternate order to reduce warm-up/order bias.
                    for workers in ([1,4] if repeat%2==0 else [4,1]):
                        server.reset_lfs_metrics();name=f"{profile}-{repeat}-{workers}"
                        started=time.perf_counter()
                        result=command(binary,"git","capture-remote",vault,name,server.url,"--work-root",work,"--include-lfs","--lfs-workers",workers,"--username",USERNAME,"--ca-file",server.certificate,secrets=secrets)
                        capture=time.perf_counter()-started
                        server.wait_lfs_idle()
                        report=json.loads(result.stdout.splitlines()[0]);assert report["lfs"]["objects"]==len(server.lfs_source_objects)
                        assert server.lfs_active==0 and 1<=server.lfs_peak<=workers
                        if workers==4:assert server.lfs_peak>=2,"No actual parallel LFS requests were observed"
                        assert len(server.lfs_transfers)==len(server.lfs_source_objects)
                        assert len({oid for method,oid in server.lfs_transfers if method=="GET"})==len(server.lfs_source_objects)
                        elapsed=server.lfs_last-server.lfs_first
                        destination=work/(name+".git");started=time.perf_counter()
                        command(binary,"git","restore",vault,name,destination,"--work-root",work)
                        restored=time.perf_counter()-started
                        actual={p.name:p.read_bytes() for p in (destination/"lfs/objects").rglob("*") if p.is_file()}
                        assert actual==server.lfs_source_objects
                        assert all(hashlib.sha256(data).hexdigest()==oid for oid,data in actual.items())
                        samples.append({"profile":profile,"repeat":repeat,"workers":workers,"objects":len(actual),"bytes":total,"injectedDelayMs":100,"peakRequests":server.lfs_peak,"requestCount":len(server.lfs_transfers),"objectSpanSeconds":round(elapsed,6),"captureSeconds":round(capture,6),"restoreSeconds":round(restored,6),"objectMiBPerSecond":round(total/(1024*1024)/elapsed,6)})
                # Corruption must stop queued work; every started worker drains before return.
                server.lfs_mode="corrupt";server.reset_lfs_metrics()
                command(binary,"git","capture-remote",vault,"rejected-"+profile,server.url,"--work-root",work,"--include-lfs","--lfs-workers",4,"--username",USERNAME,"--ca-file",server.certificate,secrets=secrets,ok=False)
                server.wait_lfs_idle()
                assert server.lfs_active==0 and server.lfs_peak<=4
                assert len(server.lfs_transfers)<=4,"Queued objects started after a failed worker"
                assert ("rejected-"+profile).encode() not in command(binary,"vault","list",vault).stdout
        encrypted_files(work)
    summary=[]
    for profile in profiles:
        serial=[s for s in samples if s["profile"]==profile and s["workers"]==1];parallel=[s for s in samples if s["profile"]==profile and s["workers"]==4]
        s=statistics.median(x["objectSpanSeconds"] for x in serial);p=statistics.median(x["objectSpanSeconds"] for x in parallel)
        summary.append({"profile":profile,"serialObjectSpanMedianSeconds":round(s,6),"parallelObjectSpanMedianSeconds":round(p,6),"objectSpanSpeedup":round(s/p,3),"serialCaptureMedianSeconds":round(statistics.median(x["captureSeconds"] for x in serial),6),"parallelCaptureMedianSeconds":round(statistics.median(x["captureSeconds"] for x in parallel),6)})
    report={"schema":1,"platform":platform.system(),"scenario":"loopback HTTPS, 100 ms delay per object, real LUKS/EFS, three alternating samples","samples":samples,"summary":summary}
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,indent=2)+"\n")
    print("LFS_BENCHMARK_JSON="+json.dumps(report,separators=(",",":")))
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"],"a",encoding="utf-8") as f:
            f.write("\n### Synthetic LFS throughput (not an Internet benchmark)\n\n| Profile | Serial span | Four-worker span | Speedup |\n|---|---:|---:|---:|\n")
            for row in summary:f.write(f'| {row["profile"]} | {row["serialObjectSpanMedianSeconds"]:.3f}s | {row["parallelObjectSpanMedianSeconds"]:.3f}s | {row["objectSpanSpeedup"]:.2f}× |\n')
if __name__=="__main__":main()
