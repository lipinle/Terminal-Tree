#!/usr/bin/env python3
"""Synthetic, same-filesystem, warm-cache transaction benchmark; no speed gates."""
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile

ROOT=Path(__file__).resolve().parents[2]
PYTHON="""import os, sys, time
sys.path.insert(0,sys.argv[1])
os.environ['XDG_STATE_HOME']=sys.argv[3]
import tree
root=sys.argv[2]
src=os.path.join(root,'source')
dst=os.path.join(root,'destination')
t=time.perf_counter()
op=tree.copy_item if sys.argv[4]=='copy' else tree.move_item
op(src,dst,tree.identity(src))
assert not tree.recovery_records()
print((time.perf_counter()-t)*1000)
"""

def digest(p):
    with p.open("rb") as f:
        return hashlib.file_digest(f,"sha256").hexdigest()

def snapshot(p):
    if p.is_file():
        return {"file":digest(p)}
    return {str(x.relative_to(p)):digest(x) for x in p.rglob("*") if x.is_file()}

def make(root,shape):
    if shape=="file32MiB":
        with root.open("wb") as f:
            for _ in range(32): f.write(bytes(range(256))*4096)
            f.flush(); os.fsync(f.fileno())
    else:
        root.mkdir()
        for i in range(500):
            with (root/f"{i:04}.txt").open("wb") as f:
                f.write(bytes(range(256))*16)
                f.flush(); os.fsync(f.fileno())
        fd=os.open(root,os.O_DIRECTORY); os.fsync(fd); os.close(fd)
    fd=os.open(root.parent,os.O_DIRECTORY); os.fsync(fd); os.close(fd)

def once(binary,base,shape,kind,lang,number):
    fixture=base/f"{shape}-{kind}-{lang}-{number}"
    fixture.mkdir()
    work=fixture/"work"; work.mkdir()
    state=fixture/"state"; state.mkdir(mode=0o700)
    journal=state/"terminal-tree"/"operations"; journal.mkdir(parents=True,mode=0o700)
    source=work/"source"; destination=work/"destination"
    make(source,shape)
    before=snapshot(source)
    cmd=([binary,str(work),str(journal),kind] if lang in ("rust","baseline") else
         [sys.executable,"-c",PYTHON,str(ROOT/"src"),str(work),str(state),kind])
    result=subprocess.run(cmd,check=True,capture_output=True,text=True)
    assert snapshot(destination)==before
    if kind=="copy": assert snapshot(source)==before
    else: assert not source.exists()
    phases={}
    for line in result.stderr.splitlines():
        if line.startswith("TT_PHASES "):
            phases=json.loads(line.removeprefix("TT_PHASES "))
    return float(result.stdout.strip()), phases

def main(binary,baseline=None):
    result={}
    profile={}
    comparison={}
    with tempfile.TemporaryDirectory(prefix="tt-ops-benchmark-") as tmp:
        base=Path(tmp)
        for shape in ("file32MiB","tree500"):
            for kind in ("copy","move"):
                samples={"python":[],"rust":[]}
                phase_samples={}
                if baseline: samples["baseline"]=[]
                for i in range(3):
                    order=list(samples)
                    order=order[i%len(order):]+order[:i%len(order)]
                    for lang in order:
                        # Baseline uses the old Rust binary but distinct fixtures.
                        elapsed,phases=once(baseline if lang=="baseline" else binary,base,shape,kind,lang,i)
                        samples[lang].append(elapsed)
                        if lang=="rust":
                            for k,v in phases.items():phase_samples.setdefault(k,[]).append(v)
                p,r=[statistics.median(samples[l]) for l in ("python","rust")]
                key=f"{shape}_{kind}"
                result[key]={"python_ms":round(p,3),"rust_ms":round(r,3),"python_over_rust":round(p/r,2)}
                profile[key]={k:round(statistics.median(v),3) for k,v in phase_samples.items()}
                if baseline:
                    old=statistics.median(samples["baseline"])
                    comparison[key]={"baseline_ms":round(old,3),"current_ms":round(r,3),"baseline_over_current":round(old/r,3)}
    print("TT_OPS_BENCHMARK "+json.dumps(result,sort_keys=True))
    print("TT_PHASE_BENCHMARK "+json.dumps(profile,sort_keys=True))
    if baseline: print("TT_SAME_RUN_BENCHMARK "+json.dumps(comparison,sort_keys=True))

if __name__=="__main__": main(str(Path(sys.argv[1]).resolve()),str(Path(sys.argv[2]).resolve()) if len(sys.argv)>2 else None)
