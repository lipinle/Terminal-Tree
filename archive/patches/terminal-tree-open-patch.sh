#!/usr/bin/env bash
set -eu
python3 - <<'PY'
import os, ast, shutil, tempfile, stat
from pathlib import Path
from datetime import datetime
p=Path(os.environ.get('XDG_DATA_HOME') or Path.home()/'.local/share')/'terminal-tree'/'tree.py'
if not p.is_file(): raise SystemExit(f'Terminal Tree not found: {p}')
p=p.resolve()
s=p.read_text(encoding='utf-8')
old="                        subprocess.Popen(['xdg-open',p],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True); return self.result"
new="""                        subprocess.Popen(['xdg-open',p],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
                        self.msg='Opening: '+safe(os.path.basename(p))
                        continue"""
if s.count(new)==1 and old not in s:
    print('Open-file patch already installed.'); raise SystemExit(0)
if s.count(old)!=1 or 'if p and os.path.isfile(p):' not in s or 'if p and isdir(p): self.go(p); return self.result' not in s:
    raise SystemExit('Unrecognized Terminal Tree version. No changes made.')
s=s.replace(old,new,1).replace('Enter open/exit','Enter open | Esc exit').replace('Enter open and return to shell','Enter open file / cd and exit on folder').replace('Open item and return to shell','Open file; on folder, cd and exit')
compile(s,str(p),'exec')
backup=p.with_name('tree.py.open-backup.'+datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
shutil.copy2(p,backup)
fd,tmp=tempfile.mkstemp(prefix='.tree-open-',dir=p.parent)
try:
    with os.fdopen(fd,'w',encoding='utf-8') as f: f.write(s)
    os.chmod(tmp,stat.S_IMODE(p.stat().st_mode))
    os.replace(tmp,p)
finally:
    if os.path.exists(tmp): os.unlink(tmp)
print('Open-file patch installed. Enter opens files without exiting the navigator.')
print('Backup:',backup)
PY
