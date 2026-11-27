import os, sys, shutil, subprocess, tempfile, time, re, difflib, stat
from pathlib import Path
ROOT=Path(__file__).resolve().parent
HOME=Path.home(); RC=HOME/'.bashrc'; DATA=Path(os.environ.get('XDG_DATA_HOME',str(HOME/'.local/share')))/'terminal-tree'
BEGIN='# >>> terminal-tree managed >>>'; END='# <<< terminal-tree managed <<<'
BLOCK=BEGIN+'\n'+'''if [[ -f "${XDG_DATA_HOME:-$HOME/.local/share}/terminal-tree/integration.sh" ]]; then
    source "${XDG_DATA_HOME:-$HOME/.local/share}/terminal-tree/integration.sh"
fi'''+ '\n'+END

def sync_directory(p):
    fd=os.open(p,os.O_RDONLY|os.O_DIRECTORY)
    try: os.fsync(fd)
    finally: os.close(fd)

def regular_or_missing(p):
    try: st=p.lstat()
    except FileNotFoundError: return
    if not stat.S_ISREG(st.st_mode):
        raise SystemExit('Refusing nonregular or symbolic-link managed file: '+str(p))

def atomic(p,b):
    regular_or_missing(p)
    p.parent.mkdir(parents=True,exist_ok=True)
    fd,t=tempfile.mkstemp(prefix='.'+p.name+'.',dir=p.parent)
    try:
        with os.fdopen(fd,'wb') as f:
            f.write(b)
            if p.exists(): os.fchmod(f.fileno(),p.stat().st_mode & 0o777)
            f.flush(); os.fsync(f.fileno())
        sync_directory(p.parent)
        os.replace(t,p)
        sync_directory(p.parent)
    finally:
        if os.path.exists(t): os.unlink(t)

def validate(s):
    r=subprocess.run(['bash','-n'],input=s.encode('utf-8','surrogateescape'),stderr=subprocess.PIPE)
    if r.returncode: raise SystemExit('Bash syntax check failed; .bashrc unchanged:\n'+r.stderr.decode(errors='replace'))

def backup_file(source,dest):
    # Exclusive backup, never overwrite an earlier backup or follow a destination link.
    sfd=os.open(source,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    with os.fdopen(sfd,'rb') as inp:
        if not stat.S_ISREG(os.fstat(inp.fileno()).st_mode): raise SystemExit('Refusing nonregular backup source')
        fd=os.open(dest,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
        with os.fdopen(fd,'wb') as out:
            shutil.copyfileobj(inp,out); out.flush(); os.fsync(out.fileno())
    sync_directory(dest.parent)

def backup():
    if RC.exists():
        b=RC.with_name('.bashrc.terminal-tree-backup.'+time.strftime('%Y%m%d-%H%M%S')+'.'+str(os.getpid()))
        backup_file(RC,b); print('Backup:',b)

def main():
    action=sys.argv[1] if len(sys.argv)>1 else 'install'
    if action not in ('install','--uninstall','--help'): raise SystemExit('Usage: bash install.sh [--uninstall|--help]')
    if action=='--help': print('Install: bash install.sh\nUninstall: bash install.sh --uninstall'); return
    if os.geteuid()==0: raise SystemExit('Run as your normal desktop user, not root / sudo.')
    regular_or_missing(RC)
    if DATA.is_symlink() or (DATA.exists() and not DATA.is_dir()):
        raise SystemExit('Refusing non-directory or symbolic-link runtime directory')
    for name in ('tree.py','integration.sh','previous-block.txt'):
        regular_or_missing(DATA/name)
    old=RC.read_bytes().decode('utf-8','surrogateescape') if RC.exists() else ''
    pat=re.compile(r'(?m)^'+re.escape(BEGIN)+r'\n[\s\S]*?^'+re.escape(END)+r'(?=\n|$)')
    matches=list(pat.finditer(old)); marker=bool(matches)
    if len(matches)>1 or (not marker and (BEGIN in old or END in old)): raise SystemExit('Malformed managed block; no changes made.')
    saved=DATA/'previous-block.txt'
    if action=='--uninstall':
        if not marker: print('No managed block found; nothing to uninstall.'); return
        if not saved.exists(): raise SystemExit('Previous-block record missing; .bashrc unchanged.')
        prior=saved.read_bytes().decode('utf-8','surrogateescape'); new=pat.sub(lambda _:prior,old,count=1); validate(new); backup(); atomic(RC,new.encode('utf-8','surrogateescape'))
        for f in ['integration.sh','tree.py','previous-block.txt']:
            try: (DATA/f).unlink()
            except FileNotFoundError: pass
        print('Uninstalled. Close and reopen Terminal. Settings retained.'); return
    import curses
    if not shutil.which('python3'): raise SystemExit('Python 3 not installed.')
    if marker:
        if not saved.exists(): raise SystemExit('Previous-block record missing; cannot safely upgrade.')
        new=pat.sub(lambda _:BLOCK,old,count=1)
    else:
        original=(ROOT/'archive/legacy/original-tree.sh').read_bytes().decode('utf-8','surrogateescape').strip('\n')
        # Recognize the original block despite comment/blank-line/indent changes.
        # Never remove an unidentified cd/clear implementation.
        lines=old.splitlines(keepends=True)
        src_lines=original.splitlines()
        norm=lambda x:x.strip()
        def meaningful(items):
            return [(i,norm(x)) for i,x in enumerate(items)
                    if norm(x) and not norm(x).startswith('#')]
        src=[x for _,x in meaningful(src_lines)]
        actual=meaningful(lines)
        words=[x for _,x in actual]
        hits=[]
        for j,x in enumerate(words):
            if x==src[0] and words[j:j+len(src)]==src:
                hits.append((actual[j][0],actual[j+len(src)-1][0]+1))
        # The user's actual .bashrc uses the older _print_branch version.
        # Match its entire executable block, not merely the function names.
        legacy_lines=(ROOT/'archive/legacy/print-branch.sh').read_text(encoding='utf-8').strip('\n').splitlines()
        legacy_src=[x for _,x in meaningful(legacy_lines)]
        legacy_hits=[(actual[j][0],actual[j+len(legacy_src)-1][0]+1)
                     for j,x in enumerate(words)
                     if x==legacy_src[0] and words[j:j+len(legacy_src)]==legacy_src]
        if len(hits)+len(legacy_hits)>1:
            raise SystemExit('Multiple original blocks found; no changes made.')
        if legacy_hits:
            hits=legacy_hits
            src_lines=legacy_lines
            src=legacy_src
            print('Recognized _print_branch legacy version.')
        if not hits:
            # A modified version may be migrated only after an explicit review.
            # Require all known top-level function signatures, the original
            # startup sequence, and high similarity. Do not guess boundaries.
            starts=[j for j,x in enumerate(words) if x==src[0]]
            tail=src[-5:]
            ends=[j+len(tail) for j in range(len(words)-len(tail)+1) if words[j:j+len(tail)]==tail]
            headers=[x for x in src if re.match(r'^(?:_build_tree_data|show_my_computer|_tree_nav|cd|lsx|clear)\(\) \{$',x)]
            candidates=[]
            for j in starts:
                for k in ends:
                    if k<=j: continue
                    segment=words[j:k]
                    found=[x for x in segment if x in headers]
                    if found!=headers: continue
                    if difflib.SequenceMatcher(None,src,segment,autojunk=False).ratio()>=0.90:
                        candidates.append((actual[j][0],actual[k-1][0]+1,segment))
            if len(candidates)==1:
                i,k,segment=candidates[0]
                print('Recognized a modified legacy Terminal Tree block at lines %d-%d.'%(i+1,k))
                print('Review executable-line differences from the original:')
                print('\n'.join(difflib.unified_diff(src,segment,fromfile='original',tofile='your .bashrc',lineterm='')))
                try:
                    with open('/dev/tty','w') as tty:
                        tty.write('Replace this entire legacy block after making a backup? [y/N] '); tty.flush()
                    with open('/dev/tty','r') as tty:
                        answer=tty.readline().strip().lower()
                except OSError: answer=''
                if answer not in ('y','yes'): raise SystemExit('Cancelled. No changes made.')
                hits=[(i,k)]
        if hits:
            i,k=hits[0]
            if i>0 and norm(lines[i-1])==norm(src_lines[0]): i-=1
            before=''.join(lines[:i]); part=''.join(lines[i:k]); after=''.join(lines[k:])
            prior=part.rstrip('\r\n'); suffix=part[len(prior):]
            new=before+BLOCK+suffix+after
            # Do not install if there are additional unrelated overrides.
            remaining=before+after
            print('Legacy Terminal Tree detected at lines %d-%d.'%(i+1,k))
        else:
            prior=''; remaining=old; new=old+('' if not old or old.endswith('\n') else '\n')+'\n'+BLOCK+'\n'
        conflict=re.compile(r'^\s*(?:function\s+)?(?:_print_branch|_build_tree_data|_tree_nav|show_my_computer|lsx|cd|clear)\s*(?:\(\s*\))?\s*\{')
        problems=[(i+1,l.strip()) for i,l in enumerate(old.splitlines()) if conflict.search(l) and (not hits or not (hits[0][0]<=i<hits[0][1]))]
        if problems:
            for n,line in problems: print(f'Conflicting definition at .bashrc:{n}: {line}')
            raise SystemExit('Unrecognized or additional legacy functions. No changes made. Send your .bashrc for an exact migration.')
        if re.search(r'(?m)^\s*alias\s+(?:cd|clear|lsx)=',old): raise SystemExit('Conflicting alias in .bashrc; no changes made.')
    program=(ROOT/'src/tree.py').read_bytes(); integration=(ROOT/'shell/integration.sh').read_bytes()
    compile(program,'tree.py','exec'); validate(integration.decode('utf-8'))
    validate(new); backup(); DATA.mkdir(parents=True,exist_ok=True)
    if marker:
        for name in ('tree.py','integration.sh'):
            path=DATA/name
            if path.is_file():
                b=DATA/(name+'.upgrade-backup.'+time.strftime('%Y%m%d-%H%M%S')+'.'+str(os.getpid()))
                backup_file(path,b); print('Runtime backup:',b)
    if not marker: atomic(saved,prior.encode('utf-8','surrogateescape'))
    atomic(DATA/'tree.py',program); atomic(DATA/'integration.sh',integration); atomic(RC,new.encode('utf-8','surrogateescape'))
    print('Installed Terminal Tree v2.1.1. Close and reopen Terminal. Ctrl+Down opens navigator; F1 for help.')
    if not shutil.which('eza'): print('eza not found: built-in colors and icons will be used.')
    if not shutil.which('gio'): print('Warning: gio not available; safe trash operation disabled.')
    if not shutil.which('findmnt'): print('Warning: findmnt not available; mount discovery disabled.')
    if not shutil.which('xdg-open'): print('Warning: xdg-open not available; external file opening disabled.')
main()
