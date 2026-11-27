#!/usr/bin/env python3
"""Shell tree and interactive file navigator; Python standard library only."""
import argparse, curses, json, os, shutil, stat, subprocess, sys, unicodedata, re
from functools import lru_cache
from pathlib import Path
from urllib.parse import unquote
from datetime import datetime

HOME = os.path.expanduser('~')
TRASH = os.path.join(os.environ.get('XDG_DATA_HOME', os.path.join(HOME,'.local/share')), 'Trash')
TF = os.path.join(TRASH,'files')
CONF = os.path.join(os.environ.get('XDG_CONFIG_HOME',os.path.join(HOME,'.config')),'terminal-tree','settings.json')

def safe(s):
    return ''.join(c if c.isprintable() and unicodedata.category(c) not in ('Cf','Cs') else ascii(c)[1:-1] for c in str(s))
def width(c):
    return 0 if unicodedata.combining(c) else (2 if unicodedata.east_asian_width(c) in ('W','F') else 1)
def clip(s,n):
    out=''; w=0
    for c in s:
        k=width(c)
        if w+k>n: break
        out+=c; w+=k
    return out

def inside(p,root):
    try: return os.path.commonpath([p,root])==root
    except ValueError: return False

def exists(p): return os.path.lexists(p)
def isdir(p): return os.path.isdir(p)
SGR = re.compile(r'\x1b\[([0-9;]*)m')
RESET = '\x1b[0m'
EZA = shutil.which('eza')

def private(c): return 0xe000<=ord(c)<=0xf8ff or 0xf0000<=ord(c)<=0xffffd or 0x100000<=ord(c)<=0x10fffd
def safe_style(s): return ''.join(c if private(c) else safe(c) for c in s)

def paint(text, code): return f'\x1b[{code}m{text}{RESET}'

def fallback(p):
    ext=os.path.splitext(p)[1].lower()
    if os.path.islink(p): glyph,col='↗','36'
    elif isdir(p): glyph,col='📁','1;34'
    elif ext in ('.png','.jpg','.jpeg','.gif','.webp','.svg','.bmp','.ico','.avif'): glyph,col='▧','35'
    elif ext in ('.zip','.gz','.xz','.7z','.tar','.rar','.bz2','.zst'): glyph,col='▣','31'
    elif ext in ('.mp3','.flac','.ogg','.wav','.m4a'): glyph,col='♫','36'
    elif ext in ('.mp4','.mkv','.webm','.avi','.mov'): glyph,col='▶','35'
    elif ext in ('.py','.rs','.sh','.js','.ts','.c','.cpp','.h','.java','.go','.kt','.rb','.html','.css'): glyph,col='⌘','33'
    elif ext in ('.json','.toml','.yaml','.yml','.ini','.conf','.xml'): glyph,col='⚙','33'
    elif ext in ('.pdf','.doc','.docx','.odt','.txt','.md','.rtf'): glyph,col='▤','37'
    elif ext in ('.xls','.xlsx','.csv','.ods'): glyph,col='▦','32'
    elif ext in ('.deb','.rpm','.appimage','.exe') or os.access(p,os.X_OK): glyph,col='◆','1;32'
    else: glyph,col='·','37'
    return paint(glyph+' '+safe(os.path.basename(p)),col)

@lru_cache(maxsize=24)
def eza_batch(parent,names):
    # This is presentation only. Never derive a filesystem path from eza output.
    found={}
    if not EZA or not names: return found
    env=os.environ.copy(); env.pop('NO_COLOR',None)
    try:
        out=subprocess.run([EZA,'--oneline','--list-dirs','--icons=always','--color=always','--sort=none','--',*names],cwd=parent,env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=2,check=True).stdout.decode('utf-8','surrogateescape')
    except (OSError,subprocess.SubprocessError): return found
    wanted=set(names)
    for line in out.split('\n'):
        plain=SGR.sub('',line)
        if not plain or safe_style(plain)!=plain: continue
        # eza prefixes the icon; accept only an exact, unambiguous filename.
        m=re.fullmatch(r'([^\s]+)[ \t]+(.+)',plain)
        if not m: continue
        name=m.group(2)
        if name in wanted and any(private(c) for c in m.group(1)):
            found[name]=line+RESET
    return found

def decorations(parent,paths):
    # Small batches keep command line size bounded, even with long names.
    out={}; names=[os.path.basename(p) for p in paths if safe(os.path.basename(p))==os.path.basename(p) and not os.path.basename(p).startswith((' ','\t'))]
    for i in range(0,len(names),256): out.update(eza_batch(parent,tuple(names[i:i+256])))
    return out

def rgb_index(r,g,b):
    if curses.COLORS<256:
        return (1 if r>127 else 0)+(2 if g>127 else 0)+(4 if b>127 else 0)
    levels=(0,95,135,175,215,255)
    q=lambda v:min(range(6),key=lambda i:abs(levels[i]-v))
    cube=16+36*q(r)+6*q(g)+q(b)
    grey=max(0,min(23,round(((r+g+b)/3-8)/10)))
    if sum((v-(8+10*grey))**2 for v in (r,g,b))<sum((v-levels[q(v)])**2 for v in (r,g,b)): return 232+grey
    return cube

class Theme:
    def __init__(self): self.pairs={}; self.bg=-1
    def init(self):
        if curses.has_colors():
            curses.start_color()
            try: curses.use_default_colors()
            except curses.error: self.bg=curses.COLOR_BLACK
    def attr(self,fg,bold,dim,underline):
        a=(curses.A_BOLD if bold else 0)|(curses.A_DIM if dim else 0)|(curses.A_UNDERLINE if underline else 0)
        if not curses.has_colors() or fg is None: return a
        if fg>=curses.COLORS: fg=fg%8
        if fg not in self.pairs:
            n=len(self.pairs)+1
            if n>=min(curses.COLOR_PAIRS,256): return a
            try: curses.init_pair(n,fg,self.bg)
            except curses.error: return a
            self.pairs[fg]=n
        return a|curses.color_pair(self.pairs[fg])
    def spans(self,label):
        fg=None; bold=dim=underline=False; pos=0
        for m in SGR.finditer(label):
            if m.start()>pos: yield label[pos:m.start()],self.attr(fg,bold,dim,underline)
            codes=[int(x or 0) for x in m.group(1).split(';')]; i=0
            while i<len(codes):
                c=codes[i]; i+=1
                if c==0: fg=None; bold=dim=underline=False
                elif c==1: bold=True
                elif c==2: dim=True
                elif c==4: underline=True
                elif c==22: bold=dim=False
                elif c==24: underline=False
                elif 30<=c<=37: fg=c-30
                elif 90<=c<=97: fg=c-90+8
                elif c==39: fg=None
                elif c in (38,48,58) and i<len(codes):
                    mode=codes[i]; i+=1
                    if mode==5 and i<len(codes):
                        if c==38: fg=max(0,min(255,codes[i]))
                        i+=1
                    elif mode==2 and i+2<len(codes):
                        if c==38 and curses.has_colors(): fg=rgb_index(*[max(0,min(255,v)) for v in codes[i:i+3]])
                        i+=3
            pos=m.end()
        if pos<len(label): yield label[pos:],self.attr(fg,bold,dim,underline)

def mounts():
    result=[]
    try:
        data=json.loads(subprocess.check_output(['findmnt','-J','-l','-o','TARGET,SOURCE,FSTYPE'],stderr=subprocess.DEVNULL,timeout=2))
        for d in data.get('filesystems',[]):
            p=d.get('target',''); src=d.get('source',''); fs=d.get('fstype','')
            if p!='/' and isdir(p) and (p.startswith(('/media/','/mnt/','/run/media/')) or (src.startswith('/dev/') and not src.startswith('/dev/loop') and not inside(p,'/boot')) or fs in ('nfs','nfs4','cifs','sshfs','fuse.sshfs')):
                if p not in result: result.append(p)
    except (OSError,ValueError,subprocess.SubprocessError): pass
    return sorted(result)

def scan(p,hidden=False,order='name',query=''):
    with os.scandir(p) as it:
        entries=[e for e in it if (hidden or not e.name.startswith('.')) and query.casefold() in e.name.casefold()]
    def key(e):
        try:
            d=e.is_dir()
            if order in ('size','date'):
                st=e.stat(follow_symlinks=False); v=-st.st_size if order=='size' else -st.st_mtime
            else: v=e.name.casefold() if order=='name' else os.path.splitext(e.name)[1].casefold()
        except OSError: d=False; v='' if order in ('name','type') else 0
        return (not d,v,e.name.casefold(),e.name)
    return [e.path for e in sorted(entries,key=key)]

def children(p,hidden,order='name',query=''):
    try: return scan(p,hidden,order,query),''
    except OSError as e: return [],str(e)

def settings():
    try:
        with open(CONF) as f: d=json.load(f)
        if not isinstance(d,dict): return {}
        return d
    except (OSError,ValueError): return {}

def save_settings(hidden,order):
    os.makedirs(os.path.dirname(CONF),exist_ok=True)
    tmp=CONF+'.tmp.'+str(os.getpid())
    with open(tmp,'w') as f: json.dump({'hidden':hidden,'order':order},f)
    os.replace(tmp,CONF)

def tree(p,hidden,order='name',query='',preview=None,devices=None,cached=None):
    devices=mounts() if devices is None else devices
    rows=[]
    def add(label,path=None): rows.append((label,path))
    def entries(path,indent,select=True,limit=None,filter_text=''):
        vals,err=(cached(path,filter_text) if cached else children(path,hidden,order,filter_text))
        if err: add(indent+'└── ! '+safe(err)); return
        shown=vals if limit is None else vals[:limit]
        styled=decorations(path,shown)
        for i,v in enumerate(shown):
            last=i==len(vals)-1; branch='└── ' if last else '├── '
            add(paint(indent+branch,'2;37')+(styled.get(os.path.basename(v)) or fallback(v)),v if select else None)
            if select and v==preview and isdir(v): entries(v,indent+('    ' if last else '│   '),False,15)
        if len(shown)<len(vals): add(indent+'└── … '+str(len(vals)-len(shown))+' more')
        if not vals: add(indent+'└── (empty)')
    roots=[('Home',HOME)]+[('Device: '+safe(d),d) for d in devices]+[('File System','/'),('Trash',TF)]
    owner=TF if inside(p,TF) else max([r for _,r in roots if r!=TF and inside(p,r)],key=len,default='/')
    add(paint('💻 My Computer','1;36'))
    for i,(label,r) in enumerate(roots):
        last=i==len(roots)-1; branch='└── ' if last else '├── '; indent='    ' if last else '│   '
        if r==TF:
            try:
                with os.scandir(TF) as it: label+=f' [{sum(1 for _ in it)}]'
            except OSError: label+=' [0 / unavailable]'
        glyph,col=('🏠','1;34') if r==HOME else ('🗑','1;33') if r==TF else ('💽','1;36') if r=='/' else ('🔌','1;32')
        add(paint(branch,'2;37')+paint(glyph+' '+label,col),r)
        if r==owner:
            if p!=r: add(paint(indent+'└── ','2;37')+paint('📁 '+safe(os.path.relpath(p,r)),'1;34'),p); indent+='    '
            entries(p,indent,True,filter_text=query)
        elif preview==r and isdir(r): entries(r,indent,False,15)
    return rows

# Terminal Tree data safety v1 (Linux).
import ctypes
import errno
import tempfile
import hashlib
import uuid
from contextlib import contextmanager

def sync_directory(path):
    fd=os.open(path,os.O_RDONLY|os.O_DIRECTORY)
    try: os.fsync(fd)
    finally: os.close(fd)

def state_directory():
    return os.path.join(os.environ.get('XDG_STATE_HOME') or os.path.join(HOME,'.local','state'),'terminal-tree','operations')

def managed_locations():
    return [os.path.realpath(p) for p in (state_directory(),
        os.path.join(os.environ.get('XDG_DATA_HOME') or os.path.join(HOME,'.local','share'),'terminal-tree'),
        os.path.dirname(CONF))]

def journal_directory():
    path=state_directory()
    os.makedirs(path,mode=0o700,exist_ok=True)
    st=os.lstat(path)
    if not stat.S_ISDIR(st.st_mode) or st.st_uid!=os.getuid() or stat.S_IMODE(st.st_mode)&0o077:
        raise ValueError('Operation journal must be a private directory owned by you')
    sync_directory(os.path.dirname(path))
    sync_directory(os.path.dirname(os.path.dirname(path)))
    return path

@contextmanager
def operation_log(kind,src=None,dst=None,stage=None):
    directory=journal_directory()
    ident=uuid.uuid4().hex
    pending=os.path.join(directory,ident+'.pending.json')
    completed=os.path.join(directory,ident+'.done.json')
    record={'id':ident,'operation':kind,'source':src,'destination':dst,'stage':stage,
            'started':datetime.now().astimezone().isoformat()}
    fd=os.open(pending,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
    with os.fdopen(fd,'w',encoding='utf-8') as f:
        json.dump(record,f,ensure_ascii=True); f.write('\n'); f.flush(); os.fsync(f.fileno())
    sync_directory(directory)
    # Exceptions, including interrupts, deliberately leave a pending record.
    yield record
    os.replace(pending,completed)
    sync_directory(directory)

def recovery_records():
    path=state_directory()
    if not os.path.exists(path): return []
    path=journal_directory()
    out=[]
    for name in sorted(os.listdir(path)):
        if not name.endswith('.pending.json'): continue
        p=os.path.join(path,name)
        try:
            fd=os.open(p,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
            with os.fdopen(fd,'r',encoding='utf-8') as f:
                st=os.fstat(f.fileno())
                if not stat.S_ISREG(st.st_mode) or st.st_uid!=os.getuid() or st.st_size>1024*1024:
                    raise ValueError('Invalid journal record')
                rec=json.loads(f.read(1024*1024+1))
            if not isinstance(rec,dict): raise ValueError('Invalid journal record')
            out.append(rec)
        except (OSError,ValueError):
            out.append({'unreadable_record':p})
    return out

def identity(p):
    st=os.lstat(p)
    parent=os.stat(os.path.dirname(p) or '.')
    return (st.st_dev,st.st_ino,st.st_mode,st.st_size,st.st_mtime_ns,st.st_ctime_ns,parent.st_dev,parent.st_ino)

def unchanged(p,expected):
    if expected is None or identity(p)!=expected:
        raise ValueError('Item changed or not verified; refresh and select again: '+safe(p))

def all_mounts():
    # Includes bind mounts and nested mounts, not just visible device shortcuts.
    with open('/proc/self/mountinfo','rb') as f:
        return [os.fsdecode(re.sub(rb'\\([0-7]{3})',lambda m:bytes([int(m[1],8)]),line.split()[4])) for line in f if line.strip()]

def physical_entry(p):
    # Do not follow the final symlink: deleting a link must not delete its target.
    return os.path.join(os.path.realpath(os.path.dirname(p)),os.path.basename(p))

def guard_source(p, destructive=True):
    p=os.path.abspath(p); real=physical_entry(p)
    if not os.path.basename(p): raise ValueError('Root is protected')
    protected=[os.path.realpath(HOME),os.path.realpath(TRASH),os.path.realpath(TF),
               '/bin','/boot','/dev','/etc','/lib','/lib64','/proc','/root','/run','/sbin','/sys','/usr','/var']
    if destructive and any(inside(real,r) or inside(r,real) for r in managed_locations()):
        raise ValueError('Terminal Tree state and runtime are protected: '+safe(p))
    if any(real==r or inside(r,real) for r in protected): raise ValueError('Protected location: '+safe(p))
    if destructive and any(inside(real,r) for r in protected[3:]): raise ValueError('System location is protected: '+safe(p))
    if destructive and inside(real,os.path.realpath(TRASH)) and os.path.dirname(real)!=os.path.realpath(TF):
        raise ValueError('Only top-level Trash items can be modified')
    if any(inside(m,real) for m in all_mounts()): raise ValueError('Mount point or directory containing a mount: '+safe(p))
    st=os.lstat(p)
    if not (stat.S_ISREG(st.st_mode) or stat.S_ISDIR(st.st_mode) or stat.S_ISLNK(st.st_mode)):
        raise ValueError('Special file is not supported: '+safe(p))

def guard_destination(p):
    real=os.path.realpath(p)
    if real=='/' or any(inside(real,r) for r in ('/bin','/boot','/dev','/etc','/lib','/lib64','/proc','/root','/run','/sbin','/sys','/usr','/var',os.path.realpath(TRASH))):
        raise ValueError('Protected destination: '+safe(p))
    if any(inside(real,r) for r in managed_locations()):
        raise ValueError('Terminal Tree state and runtime are protected')
    if not os.path.isdir(p): raise ValueError('Destination directory unavailable')

def dir_identity(p):
    st=os.stat(p)
    return st.st_dev,st.st_ino

def check_dir(p,expected):
    if dir_identity(p)!=expected: raise ValueError('Destination directory changed; operation cancelled')

def nonconflict(p):
    if exists(p): raise FileExistsError('Destination already exists: '+safe(p))

@contextmanager
def pinned_directory(path,expected):
    fd=os.open(os.path.realpath(path),os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    try:
        st=os.fstat(fd)
        if (st.st_dev,st.st_ino)!=expected: raise ValueError('Directory changed; operation cancelled')
        check_dir(path,expected)
        yield fd
    finally: os.close(fd)

def identity_at(fd,name):
    st=os.stat(name,dir_fd=fd,follow_symlinks=False); parent=os.fstat(fd)
    return (st.st_dev,st.st_ino,st.st_mode,st.st_size,st.st_mtime_ns,st.st_ctime_ns,parent.st_dev,parent.st_ino)

def rename_noreplace(src,dst,expected=None,dest_expected=None):
    libc=ctypes.CDLL(None,use_errno=True)
    fn=getattr(libc,'renameat2',None)
    if fn is None: raise RuntimeError('Atomic no-overwrite rename unavailable; operation refused')
    fn.argtypes=[ctypes.c_int,ctypes.c_char_p,ctypes.c_int,ctypes.c_char_p,ctypes.c_uint]
    fn.restype=ctypes.c_int
    expected=identity(src) if expected is None else expected
    sp=os.path.dirname(src) or '.'; dp=os.path.dirname(dst) or '.'
    dest_expected=dir_identity(dp) if dest_expected is None else dest_expected
    sn=os.path.basename(src); dn=os.path.basename(dst)
    with pinned_directory(sp,expected[-2:]) as sf, pinned_directory(dp,dest_expected) as df:
        os.fsync(sf); os.fsync(df)
        if identity_at(sf,sn)!=expected: raise ValueError('Source changed; operation cancelled')
        check_dir(sp,expected[-2:]); check_dir(dp,dest_expected)
        if fn(sf,os.fsencode(sn),df,os.fsencode(dn),1):
            e=ctypes.get_errno()
            if e==errno.EXDEV: raise ValueError('Cross-drive move refused: copy, verify, then move original to Trash')
            raise OSError(e,os.strerror(e),dst)
        # A sync error after rename is an uncertain result, not permission to retry.
        os.fsync(df); os.fsync(sf)

def tree_signature(p):
    # Refuse special files and nested mounts; detect normal concurrent edits.
    guard_source(p,False)
    result={}; todo=[p]
    while todo:
        a=todo.pop(); sig=identity(a); mode=sig[2]
        if not (stat.S_ISDIR(mode) or stat.S_ISREG(mode) or stat.S_ISLNK(mode)):
            raise ValueError('Special file inside source: '+safe(a))
        result[os.path.relpath(a,p)]=sig
        if stat.S_ISDIR(mode):
            with os.scandir(a) as it: todo.extend(e.path for e in it)
    return result

def content_manifest(root,flush=False):
    # Compare content, metadata, links and tree topology, never following final symlinks.
    out={}; links={}; dirs=[]; todo=[root]
    def key(st):
        return (st.st_dev,st.st_ino,st.st_mode,st.st_size,st.st_mtime_ns,st.st_ctime_ns)
    while todo:
        path=todo.pop(); rel=os.path.relpath(path,root)
        st=os.lstat(path); mode=st.st_mode; sig=key(st)
        try:
            names=os.listxattr(path,follow_symlinks=False)
            attrs=tuple(sorted((n,os.getxattr(path,n,follow_symlinks=False)) for n in names))
        except OSError as e:
            if e.errno not in (errno.ENOTSUP,errno.EOPNOTSUPP): raise
            attrs=()
        meta=(stat.S_IMODE(mode),st.st_uid,st.st_gid,st.st_mtime_ns,attrs)
        if stat.S_ISREG(mode):
            fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
            with os.fdopen(fd,'rb') as f:
                if key(os.fstat(f.fileno()))!=sig: raise ValueError('File changed before verification')
                digest=hashlib.sha256()
                while True:
                    buf=f.read(1024*1024)
                    if not buf: break
                    digest.update(buf)
                if key(os.fstat(f.fileno()))!=sig: raise ValueError('File changed during verification')
                if flush: os.fsync(f.fileno())
            out[rel]=('file',meta,st.st_size,digest.hexdigest())
            links.setdefault((st.st_dev,st.st_ino),[]).append(rel)
        elif stat.S_ISLNK(mode):
            out[rel]=('link',meta,os.readlink(path))
        elif stat.S_ISDIR(mode):
            out[rel]=('dir',meta); dirs.append((path,sig))
            with os.scandir(path) as it: todo.extend(e.path for e in it)
        else: raise ValueError('Special file is not supported: '+safe(path))
        if key(os.lstat(path))!=sig: raise ValueError('Entry changed during verification')
    for path,sig in reversed(dirs):
        if key(os.lstat(path))!=sig: raise ValueError('Directory changed during verification')
        if flush: sync_directory(path)
    topology=tuple(sorted(tuple(sorted(group)) for group in links.values() if len(group)>1))
    return out,topology

def copy_item(src,dst,expected=None):
    guard_source(src,False); parent=os.path.dirname(dst); guard_destination(parent); nonconflict(dst)
    expected=identity(src) if expected is None else expected
    unchanged(src,expected); dest_id=dir_identity(parent)
    if not os.path.islink(src) and isdir(src) and inside(os.path.realpath(dst),os.path.realpath(src)):
        raise ValueError('Cannot copy directory into itself')
    before=tree_signature(src); manifest=content_manifest(src)
    if tree_signature(src)!=before: raise ValueError('Source changed before copy')
    stage_name='.terminal-tree-copy-'+uuid.uuid4().hex
    if not shutil.rmtree.avoids_symlink_attacks: raise RuntimeError('Safe staging cleanup unavailable')
    with operation_log('copy',src,dst,os.path.join(parent,stage_name)):
        with pinned_directory(parent,dest_id) as fd:
            os.mkdir(stage_name,0o700,dir_fd=fd)
            stage='/proc/self/fd/'+str(fd)+'/'+stage_name
            stage_id=dir_identity(stage)
            payload=os.path.join(stage,'payload')
            try:
                subprocess.run(['cp','-a','--reflink=auto','--',src,payload],pass_fds=(fd,),
                               check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
                if content_manifest(payload,flush=True)!=manifest:
                    raise ValueError('Copy verification failed (content or metadata differs)')
                if tree_signature(src)!=before: raise ValueError('Source changed during copy; copy cancelled')
                unchanged(src,expected); check_dir(parent,dest_id); guard_destination(parent)
                rename_noreplace(payload,dst,dest_expected=dest_id)
            finally:
                st=os.stat(stage_name,dir_fd=fd,follow_symlinks=False)
                if not stat.S_ISDIR(st.st_mode) or (st.st_dev,st.st_ino)!=stage_id:
                    raise ValueError('Staging directory changed; manual recovery required')
                shutil.rmtree(stage)
                os.fsync(fd)

def move_item(src,dst,expected=None):
    guard_source(src); parent=os.path.dirname(dst); guard_destination(parent); nonconflict(dst)
    expected=identity(src) if expected is None else expected
    unchanged(src,expected); dest_id=dir_identity(parent)
    if not os.path.islink(src) and isdir(src) and inside(os.path.realpath(dst),os.path.realpath(src)):
        raise ValueError('Cannot move directory into itself')
    with operation_log('move',src,dst):
        guard_source(src); guard_destination(parent)
        rename_noreplace(src,dst,expected,dest_id)

def gio_trash(p,expected=None):
    guard_source(p)
    expected=identity(p) if expected is None else expected
    unchanged(p,expected)
    if not shutil.which('gio'): raise RuntimeError('gio not installed; no permanent-delete fallback')
    with operation_log('gio-trash',p):
        guard_source(p); unchanged(p,expected)
        subprocess.run(['gio','trash','--',p],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
        sync_directory(os.path.dirname(p))

def create_item(p,directory):
    parent=os.path.dirname(p); guard_destination(parent); dest_id=dir_identity(parent)
    nonconflict(p)
    with operation_log('mkdir' if directory else 'create-file',dst=p):
        with pinned_directory(parent,dest_id) as fd:
            name=os.path.basename(p)
            if directory: os.mkdir(name,dir_fd=fd)
            else:
                out=os.open(name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o666,dir_fd=fd)
                try: os.fsync(out)
                finally: os.close(out)
            os.fsync(fd)

def purge_item(p,expected,snapshot):
    if os.path.dirname(p)!=TF: raise ValueError('Permanent delete is only allowed directly inside home Trash')
    guard_source(p); unchanged(p,expected)
    if tree_signature(p)!=snapshot: raise ValueError('Contents changed during review; deletion cancelled')
    if not shutil.rmtree.avoids_symlink_attacks: raise RuntimeError('Safe recursive removal unavailable')
    info=os.path.join(TRASH,'info',os.path.basename(p)+'.trashinfo')
    info_sig=identity(info) if exists(info) else None
    with operation_log('permanent-delete',p):
        with pinned_directory(TF,expected[-2:]) as fd:
            guard_source(p)
            if tree_signature(p)!=snapshot or identity_at(fd,os.path.basename(p))!=expected:
                raise ValueError('Contents changed during review; deletion cancelled')
            if stat.S_ISDIR(expected[2]): shutil.rmtree('/proc/self/fd/'+str(fd)+'/'+os.path.basename(p))
            else: os.unlink(os.path.basename(p),dir_fd=fd)
            os.fsync(fd)
        if info_sig is not None:
            if not stat.S_ISREG(info_sig[2]): raise ValueError('Deleted item; nonregular Trash metadata retained')
            with pinned_directory(os.path.dirname(info),info_sig[-2:]) as fd:
                if identity_at(fd,os.path.basename(info))!=info_sig:
                    raise ValueError('Deleted item; changed Trash metadata retained')
                os.unlink(os.path.basename(info),dir_fd=fd); os.fsync(fd)

def restore_path(p):
    if os.path.dirname(p)!=TF: raise ValueError('Select an item directly under home Trash')
    info=os.path.join(TRASH,'info',os.path.basename(p)+'.trashinfo')
    fd=os.open(info,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    with os.fdopen(fd,'rb') as f:
        st=os.fstat(f.fileno())
        if not stat.S_ISREG(st.st_mode) or st.st_uid!=os.getuid() or st.st_size>65536: raise ValueError('Invalid Trash metadata')
        raw=f.read(65537).decode('utf-8','surrogateescape').splitlines()
    if not raw or raw[0]!='[Trash Info]': raise ValueError('Invalid Trash metadata')
    paths=[x[5:] for x in raw if x.startswith('Path=')]
    if len(paths)!=1: raise ValueError('Missing or ambiguous original path')
    dst=unquote(paths[0],errors='surrogateescape')
    if not os.path.isabs(dst) or '\0' in dst or os.path.normpath(dst)!=dst or dst=='/': raise ValueError('Invalid restore destination')
    guard_destination(os.path.dirname(dst)); nonconflict(dst)
    return dst,info,identity(info),dir_identity(os.path.dirname(dst))

def restore(p,expected=None,record=None):
    dst,info,sig,parent=restore_path(p)
    if record is not None and (dst,info,sig,parent)!=record: raise ValueError('Trash metadata changed; restore cancelled')
    moved=False
    try:
        with operation_log('restore',p,dst):
            check_dir(os.path.dirname(dst),parent); unchanged(info,sig)
            move_item(p,dst,expected); moved=True
            with pinned_directory(os.path.dirname(info),sig[-2:]) as fd:
                if identity_at(fd,os.path.basename(info))!=sig:
                    raise ValueError('Trash metadata changed')
                os.unlink(os.path.basename(info),dir_fd=fd); os.fsync(fd)
    except (OSError,ValueError,RuntimeError):
        if moved: return 'Restored; metadata or durability needs review (see --recovery)'
        raise
    return 'Restored'


class Nav:
    def __init__(self,s,p,hidden,order):
        self.s=s; self.p=p; self.hidden=hidden; self.order=order; self.query=''; self.mark=set(); self.clipboard=[]; self.cut=False; self.history=[p]; self.hidx=0; self.rows=[]; self.sel=0; self.top=0; self.msg=''; self.cache={}; self.devices=mounts(); self.result=p; self.undo=[]; self.theme=Theme(); self._safety_seen={}; self._safety_clip={}; self.rebuild()
        try:
            if recovery_records(): self.msg='Unresolved file operations: run terminal_tree_recovery in Bash'
        except (OSError,ValueError): self.msg='Operation journal unavailable; file changes may be refused'
    def cached(self,p,q):
        k=(p,self.hidden,self.order,q)
        if k not in self.cache:
            if len(self.cache)>=32: self.cache.clear()
            self.cache[k]=children(p,self.hidden,self.order,q)
            for a in self.cache[k][0]:
                try: self._safety_seen.setdefault(a,identity(a))
                except OSError: pass
        return self.cache[k]
    def path(self): return self.rows[self.sel][1] if self.rows else None
    def rebuild(self,keep=None,refresh=False):
        if refresh: self.mark.clear(); self._safety_seen.clear(); self.cache.clear(); eza_batch.cache_clear(); self.devices=mounts()
        if keep is None: keep=self.path()
        self.rows=tree(self.p,self.hidden,self.order,self.query,keep if keep!=self.p else None,self.devices,self.cached)
        choices=[i for i,(_,p) in enumerate(self.rows) if p is not None]
        self.sel=next((i for i in choices if self.rows[i][1]==keep),next((i for i in choices if os.path.dirname(self.rows[i][1])==self.p and self.rows[i][1]!=self.p),choices[0]))
    def put(self,y,x,t,attr=0):
        h,w=self.s.getmaxyx()
        if 0<=y<h and 0<=x<w-1:
            try: self.s.addstr(y,x,clip(safe(t),w-x-1),attr)
            except curses.error: pass
    def put_styled(self,y,x,label,extra=0):
        h,w=self.s.getmaxyx()
        for text,attr in self.theme.spans(label):
            if x>=w-1: break
            text=clip(safe_style(text),w-x-1)
            if 0<=y<h and text:
                try: self.s.addstr(y,x,text,attr|extra)
                except curses.error: pass
            x+=sum(width(c) for c in text)
    def draw(self):
        self.s.erase(); h,w=self.s.getmaxyx(); cap=max(1,h-4)
        self.top=max(0,min(self.top,max(0,len(self.rows)-cap)))
        if self.sel<self.top: self.top=self.sel
        if self.sel>=self.top+cap: self.top=self.sel-cap+1
        self.put_styled(0,0,paint('Terminal Tree','1;36')+' | '+safe(self.p))
        for y,i in enumerate(range(self.top,min(self.top+cap,len(self.rows))),1):
            label,p=self.rows[i]; a=curses.A_REVERSE if i==self.sel else 0
            self.put_styled(y,0,('* ' if p in self.mark else '  ')+label,a)
        try: free=shutil.disk_usage(self.p).free/1024**3; disk=f' | free {free:.1f} GiB'
        except OSError: disk=''
        self.put(h-3,0,f'{self.sel+1}/{len(self.rows)} | marked {len(self.mark)} | sort {self.order} | hidden {self.hidden} | filter {self.query}{disk}')
        self.put(h-2,0,self.msg or 'Wheel / click | Arrows navigate | Enter open | Esc exit | ? help | q exit')
        self.put(h-1,0,'Space mark | / filter | c/x/v copy/cut/paste | d trash | r rename | F5 refresh')
        self.s.refresh()
    def prompt(self,label):
        h,w=self.s.getmaxyx(); self.s.move(max(0,h-2),0); self.s.clrtoeol(); self.put(h-2,0,label); self.s.refresh()
        try:
            curses.mousemask(0)
            curses.echo(); curses.curs_set(1)
            v=self.s.getstr(h-2,min(len(label),max(0,w-2)),max(1,w-len(label)-2))
            return os.fsdecode(v)
        finally:
            curses.noecho()
            curses.mousemask(self.mouse_mask)
            try: curses.curs_set(0)
            except curses.error: pass
    def confirm(self,text): return self.prompt(text+' [type yes]: ')=='yes'
    def page(self,lines):
        pos=0
        while True:
            self.s.erase(); h,w=self.s.getmaxyx(); cap=max(1,h-2)
            for y,l in enumerate(lines[pos:pos+cap]): self.put(y,0,l)
            self.put(h-1,0,'↑↓ scroll | q/Esc/Enter back'); self.s.refresh(); k=self.s.get_wch()
            if k in ('q','\x1b','\n','\r',curses.KEY_ENTER): return
            if k == curses.KEY_MOUSE: pos=max(0,min(max(0,len(lines)-cap),pos+self.mouse_wheel()))
            if k in (curses.KEY_DOWN,curses.KEY_NPAGE): pos=min(max(0,len(lines)-cap),pos+(cap if k==curses.KEY_NPAGE else 1))
            if k in (curses.KEY_UP,curses.KEY_PPAGE): pos=max(0,pos-(cap if k==curses.KEY_PPAGE else 1))
    def help_page(self):
        import textwrap
        groups = (
            ('NAVIGATION', (
                ('Up / Down', 'Select previous / next item'),
                ('Mouse wheel', 'Move one item per wheel event'),
                ('Left click', 'Open file / enter folder in navigator'),
                ('Right click', 'Select without opening'),
                ('Left / Right', 'Parent / enter folder'),
                ('Enter', 'Open file; on folder, cd and exit'),
                ('Home / End', 'First / last selectable item'),
                ('PgUp / PgDn', 'Move one page'),
                ('[ / ]', 'Back / forward in history'),
                ('g', 'Go to a path'),
                ('Esc / q / Q', 'Exit navigator'),
            )),
            ('VIEW & SEARCH', (
                ('h', 'Toggle hidden files'),
                ('/', 'Filter current folder'),
                ('i', 'Show file properties'),
                ('F3 / p', 'Preview text file'),
                ('F5 / f', 'Refresh file list'),
                ('F9 / s', 'Change sort order'),
                ('S', 'Save view settings'),
            )),
            ('SELECTION & FILES', (
                ('Space', 'Toggle item selection'),
                ('a', 'Mark visible items in folder'),
                ('A', 'Clear all marks'),
                ('c', 'Copy selected items'),
                ('x', 'Cut selected items'),
                ('v', 'Review and paste into current folder'),
                ('F2 / r', 'Rename current item'),
                ('n', 'Create folder'),
                ('t', 'Create empty file'),
            )),
            ('TRASH & SAFETY', (
                ('Delete / d', 'Move selected items to Trash'),
                ('D', 'Permanently delete from home Trash only (type DELETE)'),
                ('R', 'Restore from home Trash'),
                ('u', 'Undo last rename / move in session'),
                ('F1 / ?', 'Open or close this help'),
            )),
        )
        pos = 0
        while True:
            self.s.erase()
            h, w = self.s.getmaxyx()
            heading = self.theme.attr(curses.COLOR_CYAN, True, False, False)
            key_style = self.theme.attr(curses.COLOR_YELLOW, True, False, False)
            warning = self.theme.attr(curses.COLOR_RED, True, False, False)
            dim = curses.A_DIM
            self.put(0, 0, 'TERMINAL TREE  /  KEYBOARD SHORTCUTS', heading)
            self.put(1, 0, 'Case-sensitive: a and A are different keys.', dim)
            def column(sections, col_width):
                rows = []
                key_width = 14
                for title, items in sections:
                    rows.append([(0, title, heading)])
                    for key, description in items:
                        if col_width < 34:
                            rows.extend([[(0, part, key_style)] for part in textwrap.wrap(key, max(1, col_width))])
                            rows.extend([[(2, part, 0)] for part in textwrap.wrap(description, max(1, col_width-2))])
                        else:
                            # Keep the key and action aligned, wrapping only the action.
                            action_x = key_width + 2
                            desc = textwrap.wrap(description, max(1, col_width-action_x)) or ['']
                            rows.append([(0, key, warning if key == 'D' else key_style), (action_x, desc[0], 0)])
                            rows.extend([[(action_x, part, 0)] for part in desc[1:]])
                    rows.append([])
                return rows
            wide = w >= 108
            if wide:
                left_width = (w-5)//2
                left = column(groups[:2], left_width)
                right = column(groups[2:], w-left_width-5)
                rows = [ (left[i] if i<len(left) else []) + [(x+left_width+3,t,a) for x,t,a in (right[i] if i<len(right) else [])] for i in range(max(len(left),len(right))) ]
            else:
                rows = column(groups, max(1,w-1))
            for note in ('Notes: Filter searches this folder only.', 'Drives are listed, not mounted or unmounted here.', 'Marks clear on navigation. Cross-drive cut: copy, verify, then Trash original.'):
                rows.extend([[(0, part, dim)] for part in textwrap.wrap(note,max(1,w-1))])
            cap = max(0,h-4)
            bottom = max(0,len(rows)-max(1,cap))
            pos = min(pos,bottom)
            for y, row in enumerate(rows[pos:pos+cap], 2):
                for x, text, attr in row:
                    self.put(y,x,text,attr)
            self.put(h-2,0,f'Rows {pos+1}-{min(pos+cap,len(rows))} / {len(rows)}' if cap else 'Enlarge terminal to read help',dim)
            footer = 'Up/Down: scroll | PgUp/PgDn | Esc/F1: back' if w>=47 else 'Scroll: arrows | Esc: back'
            self.put(h-1,0,footer,heading)
            self.s.refresh()
            k = self.s.get_wch()
            if k in ('q','Q','?', '\x1b','\n','\r',curses.KEY_ENTER,curses.KEY_F1): return
            if k == curses.KEY_MOUSE: pos=max(0,min(bottom,pos+self.mouse_wheel()))
            elif k == curses.KEY_DOWN: pos=min(bottom,pos+1)
            elif k == curses.KEY_UP: pos=max(0,pos-1)
            elif k == curses.KEY_NPAGE: pos=min(bottom,pos+max(1,cap))
            elif k == curses.KEY_PPAGE: pos=max(0,pos-max(1,cap))
            elif k == curses.KEY_HOME: pos=0
            elif k == curses.KEY_END: pos=bottom

    def mouse_event(self):
        try:
            _, x, y, _, state = curses.getmouse()
        except curses.error:
            return None
        return x, y, state
    def mouse_wheel(self):
        event = self.mouse_event()
        if event is None: return 0
        state = event[2]
        if state & getattr(curses, 'BUTTON4_PRESSED', 0): return -1
        if state & getattr(curses, 'BUTTON5_PRESSED', 0): return 1
        return 0
    def mouse_action(self):
        event = self.mouse_event()
        if event is None: return
        x, y, state = event
        # One real wheel event = one selectable item, not terminal-generated arrows.
        if state & getattr(curses, 'BUTTON4_PRESSED', 0):
            self.shift(-1); return
        if state & getattr(curses, 'BUTTON5_PRESSED', 0):
            self.shift(1); return
        h, w = self.s.getmaxyx()
        if h < 5 or not (0 <= x < w-1 and 1 <= y <= h-4): return
        idx = self.top + y - 1
        if not 0 <= idx < len(self.rows): return
        path = self.rows[idx][1]
        if path is None: return
        # Only react to the press; releases must not open the next row after redraw.
        if state & getattr(curses, 'BUTTON1_PRESSED', 0):
            self.sel = idx
            if path == TF and not isdir(TF): self.msg = 'Trash does not exist yet'
            elif isdir(path): self.go(path)
            elif os.path.isfile(path):
                self.rebuild(path)
                subprocess.Popen(['xdg-open',path],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
                self.msg = 'Opening: '+safe(os.path.basename(path))
            else: self.msg = 'Cannot open this item'
        elif state & getattr(curses, 'BUTTON3_PRESSED', 0):
            self.sel = idx; self.rebuild(path)

    def go(self,p,record=True,keep=None):
        if not isdir(p): raise ValueError('Not an accessible directory: '+safe(p))
        with os.scandir(p): pass
        self.p=os.path.abspath(p); self.result=self.p; self.query=''; self.top=0; self.mark.clear(); self._safety_seen.clear(); self.cache.clear()
        if record and self.history[self.hidx]!=self.p: self.history=self.history[:self.hidx+1]+[self.p]; self.hidx+=1
        self.rows=[]; self.rebuild(keep=keep)
    def shift(self,delta):
        choices=[i for i,(_,p) in enumerate(self.rows) if p is not None]; idx=choices.index(self.sel)
        self.sel=choices[(idx+delta)%len(choices)]; self.rebuild(self.path())
    def safety_review(self,title,items,word='yes'):
        # Scrollable, full paths (not just a count); explicit confirmation afterwards.
        import textwrap
        pos=0
        while True:
            self.s.erase(); h,w=self.s.getmaxyx(); cap=max(1,h-4)
            lines=[]
            for i,item in enumerate(items,1):
                lines.extend(textwrap.wrap(str(i)+'. '+repr(item),max(1,w-2),replace_whitespace=False) or [''])
            end=max(0,len(lines)-cap); pos=min(pos,end)
            self.put(0,0,title,curses.A_BOLD)
            for y,line in enumerate(lines[pos:pos+cap],1): self.put(y,0,line)
            self.put(h-3,0,f'{len(items)} item(s) | lines {pos+1}-{min(pos+cap,len(lines))}/{len(lines)}')
            self.put(h-2,0,'Review all entries; End to reach bottom')
            self.put(h-1,0,'Enter: confirm | Esc/q: cancel | arrows/PgUp/PgDn: scroll')
            self.s.refresh(); k=self.s.get_wch()
            if k in ('q','Q','\x1b'): return False
            if k in ('\n','\r',curses.KEY_ENTER):
                if pos<end: pos=min(end,pos+cap); continue
                return self.prompt('Type '+word+': ')==word
            if k==curses.KEY_DOWN: pos=min(end,pos+1)
            elif k==curses.KEY_UP: pos=max(0,pos-1)
            elif k==curses.KEY_NPAGE: pos=min(end,pos+cap)
            elif k==curses.KEY_PPAGE: pos=max(0,pos-cap)
            elif k==curses.KEY_HOME: pos=0
            elif k==curses.KEY_END: pos=end
            elif k==curses.KEY_MOUSE and hasattr(self,'mouse_wheel'): pos=max(0,min(end,pos+self.mouse_wheel()))
    def targets(self):
        selected=sorted(self.mark) if self.mark else [self.path()]
        visible={p for _,p in self.rows if p and os.path.dirname(p)==self.p and p!=self.p}
        if not selected or any(p not in visible or p in [HOME,'/',TF,*self.devices] for p in selected):
            raise ValueError('Select visible items in this folder only; clear marks with A')
        for p in selected:
            unchanged(p,self._safety_seen.get(p))
        return selected
    def operation(self,k):
        p=self.path(); targets=[]
        if k in ('c','x','\x03','\x18','r',curses.KEY_F2,'d',curses.KEY_DC,'D','R'):
            targets=self.targets()
        if k in ('c','x','\x03','\x18'):
            cut=k in ('x','\x18')
            for a in targets:
                guard_source(a,cut)
                if cut and inside(physical_entry(a),os.path.realpath(TF)): raise ValueError('Use R to restore Trash items')
            self.clipboard=targets; self._safety_clip={a:self._safety_seen[a] for a in targets}; self.cut=cut
            self.msg=f'{len(targets)} item(s) '+('cut' if cut else 'copied'); return
        if k in ('v','\x16'):
            if not self.clipboard: raise ValueError('Clipboard empty')
            guard_destination(self.p); dest_identity=dir_identity(self.p)
            pairs=[(a,os.path.join(self.p,os.path.basename(a))) for a in self.clipboard]
            # Preflight the entire batch before modifying anything.
            if len({b for a,b in pairs})!=len(pairs): raise ValueError('Duplicate destination names')
            for a,b in pairs:
                unchanged(a,self._safety_clip.get(a)); guard_source(a,self.cut); nonconflict(b)
                if self.cut and os.lstat(a).st_dev!=os.stat(self.p).st_dev: raise ValueError('Cross-drive cut refused: copy, verify, then Trash original')
                if not os.path.islink(a) and isdir(a) and inside(os.path.realpath(b),os.path.realpath(a)): raise ValueError('Cannot paste into itself')
            if not self.safety_review('Move items' if self.cut else 'Copy items',[a+' -> '+b for a,b in pairs]): return
            done=0; error=''
            for a,b in pairs:
                try:
                    check_dir(self.p,dest_identity)
                    if self.cut:
                        move_item(a,b,self._safety_clip[a]); self.undo.append((b,a,identity(b))); self.clipboard.remove(a); self._safety_clip.pop(a,None)
                    else: copy_item(a,b,self._safety_clip[a])
                    done+=1
                except (OSError,ValueError,RuntimeError,subprocess.SubprocessError) as e: error=str(e); break
            self.mark.clear(); self.msg=f'{done}/{len(pairs)} completed'+('; stopped: '+error if error else '')
        elif k in ('r',curses.KEY_F2):
            if targets!=[p] or inside(physical_entry(p),os.path.realpath(TF)): raise ValueError('Select only the current item outside Trash')
            n=self.name('New name: ')
            if n and n!=os.path.basename(p):
                dst=os.path.join(os.path.dirname(p),n)
                move_item(p,dst,self._safety_seen[p]); self.undo.append((dst,p,identity(dst))); self.mark.clear(); p=dst; self.msg='Renamed'
        elif k in ('n','t'):
            guard_destination(self.p); n=self.name('New folder: ' if k=='n' else 'New file: ')
            if n:
                p=os.path.join(self.p,n); guard_destination(self.p)
                create_item(p,k=='n')
                self.msg='Created'
        elif k in ('d',curses.KEY_DC,'D'):
            for a in targets:
                guard_source(a)
                in_trash=inside(physical_entry(a),os.path.realpath(TF))
                if k=='D' and (self.p!=TF or not in_trash): raise ValueError('Permanent delete is only allowed directly inside home Trash')
                if k!='D' and in_trash: raise ValueError('Use R to restore or D to permanently delete in Trash')
            snapshots={a:tree_signature(a) for a in targets} if k=='D' else {}
            if not self.safety_review('PERMANENT DELETE (cannot undo)' if k=='D' else 'Move to Trash',targets,'DELETE' if k=='D' else 'yes'): return
            done=0; error=''
            for a in targets:
                try:
                    expected=self._safety_seen[a]; guard_source(a); unchanged(a,expected)
                    if k=='D' and tree_signature(a)!=snapshots[a]: raise ValueError('Contents changed during review; deletion cancelled')
                    if k!='D': gio_trash(a,expected)
                    else: purge_item(a,expected,snapshots[a])
                    self.mark.discard(a); done+=1
                except (OSError,ValueError,RuntimeError,subprocess.SubprocessError) as e: error=str(e); break
            self.msg=f'{done}/{len(targets)} completed'+('; stopped: '+error if error else '')
        elif k=='R':
            records=[restore_path(a) for a in targets]
            if len({r[0] for r in records})!=len(records): raise ValueError('Duplicate restore destinations')
            if not self.safety_review('Restore from Trash',[a+' -> '+r[0] for a,r in zip(targets,records)]): return
            done=0; error=''; note=''
            for a,r in zip(targets,records):
                try:
                    note=restore(a,self._safety_seen[a],r); self.mark.discard(a); done+=1
                except (OSError,ValueError,RuntimeError,subprocess.SubprocessError) as e: error=str(e); break
            self.msg=f'{done}/{len(targets)} restored'+('; stopped: '+error if error else ('; '+note if 'metadata' in note else ''))
        elif k in ('u','\x1a'):
            if not self.undo: raise ValueError('Nothing to undo (rename/move only)')
            a,b,sig=self.undo[-1]; unchanged(a,sig); nonconflict(b); dest_identity=dir_identity(os.path.dirname(b))
            if self.safety_review('Undo last move/rename',[a+' -> '+b]):
                check_dir(os.path.dirname(b),dest_identity); move_item(a,b,sig); self.undo.pop(); self.msg='Undone'
        self.rebuild(p,True)

    def name(self,label):
        n=self.prompt(label)
        if not n: return None
        if n in ('.','..') or '/' in n or '\0' in n: raise ValueError('Enter a single filename, not a path')
        return n
    def run(self):
        try: curses.curs_set(0)
        except curses.error: pass
        self.theme.init()
        self.s.keypad(True)
        mask = (getattr(curses,'BUTTON1_PRESSED',0) | getattr(curses,'BUTTON3_PRESSED',0)
                | getattr(curses,'BUTTON4_PRESSED',0) | getattr(curses,'BUTTON5_PRESSED',0))
        curses.mouseinterval(0)
        self.mouse_mask = curses.mousemask(mask)[0]
        curses.set_escdelay(25)
        while True:
            self.draw(); k=self.s.get_wch(); self.msg=''; p=self.path()
            try:
                if k in ('q','Q','\x1b'): return self.result
                elif k==curses.KEY_MOUSE: self.mouse_action()
                elif k==curses.KEY_DOWN: self.shift(1)
                elif k==curses.KEY_UP: self.shift(-1)
                elif k in (curses.KEY_NPAGE,curses.KEY_PPAGE): self.shift(max(1,self.s.getmaxyx()[0]-4)*(1 if k==curses.KEY_NPAGE else -1))
                elif k in (curses.KEY_HOME,curses.KEY_END):
                    self.sel=next(i for i in (range(len(self.rows)) if k==curses.KEY_HOME else range(len(self.rows)-1,-1,-1)) if self.rows[i][1]); self.rebuild(self.path())
                elif k==curses.KEY_RIGHT:
                    if p==TF and not isdir(TF): self.msg='Trash does not exist yet'
                    elif p and isdir(p): self.go(p)
                elif k==curses.KEY_LEFT: self.go(os.path.dirname(self.p),keep=self.p)
                elif k in ('\n','\r',curses.KEY_ENTER):
                    if p and isdir(p): self.go(p); return self.result
                    if p and os.path.isfile(p):
                        subprocess.Popen(['xdg-open',p],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
                        self.msg='Opening: '+safe(os.path.basename(p))
                        continue
                    self.msg='Cannot open this item'
                elif k in ('h','\x08',curses.KEY_BACKSPACE): self.hidden=not self.hidden; self.mark.clear(); self._safety_seen.clear(); self.cache.clear(); self.rebuild(p)
                elif k in ('/', '\x06'): self.query=self.prompt('Filter current folder: '); self.mark.clear(); self._safety_seen.clear(); self.cache.clear(); self.rebuild()
                elif k in ('g','\x0c'):
                    v=self.prompt('Go to path: ')
                    if v: self.go(os.path.join(self.p,os.path.expanduser(v)))
                elif k=='[' and self.hidx>0: self.go(self.history[self.hidx-1],False); self.hidx-=1
                elif k==']' and self.hidx+1<len(self.history): self.go(self.history[self.hidx+1],False); self.hidx+=1
                elif k==' ' and p and p not in [HOME,'/',TF,self.p,*self.devices]:
                    if p in self.mark: self.mark.remove(p)
                    elif os.path.dirname(p)==self.p: self.mark.add(p)
                elif k in ('a','\x01'): self.mark.update(v for _,v in self.rows if v and os.path.dirname(v)==self.p and v!=self.p and v not in self.devices)
                elif k=='A': self.mark.clear()
                elif k in (curses.KEY_F5,'f'): self.rebuild(p,True)
                elif k in ('s',curses.KEY_F9):
                    orders=['name','size','date','type']; self.order=orders[(orders.index(self.order)+1)%len(orders)]; self.mark.clear(); self._safety_seen.clear(); self.cache.clear(); self.rebuild(p); self.msg='Sort: '+self.order
                elif k=='S': save_settings(self.hidden,self.order); self.msg='Settings saved'
                elif k in ('c','x','v','\x03','\x18','\x16','r',curses.KEY_F2,'n','t','d',curses.KEY_DC,'D','R','u','\x1a'): self.operation(k)
                elif k in ('i',):
                    st=os.lstat(p); self.page([p,'Mode: '+stat.filemode(st.st_mode),'Size: '+str(st.st_size)+' bytes','Modified: '+str(datetime.fromtimestamp(st.st_mtime)),'UID/GID: '+str(st.st_uid)+'/'+str(st.st_gid),'Target: '+os.readlink(p) if os.path.islink(p) else ''])
                elif k in ('p',curses.KEY_F3):
                    if not p or not os.path.isfile(p): raise ValueError('Select a regular file')
                    with open(p,'rb') as f: raw=f.read(65536)
                    if b'\0' in raw: self.page([p,'Binary file; use Enter to open externally'])
                    else: self.page([p]+raw.decode('utf-8','replace').splitlines()+['[Preview limited to 64 KiB]'])
                elif k in ('?',curses.KEY_F1): self.help_page()
            except (OSError,ValueError,RuntimeError,subprocess.SubprocessError) as e: self.msg=str(e)

HELP=['Terminal Tree — keys','↑ ↓ select; ← parent; → enter folder; Enter open file / cd and exit on folder','q / Esc exit at current folder; Home/End; Page Up/Down','h hidden; / filter current directory; g go to path; [ ] history','Space mark; a mark visible children; A clear marks; c copy; x cut; v paste','r/F2 rename; n new folder; t new file; d/Delete trash; D permanent delete from home Trash only (type DELETE)','R restore home Trash item; u undo last rename/move (session only)','i properties; p/F3 text preview; f/F5 refresh; s/F9 sort; S save settings','? / F1 help. Search is nonrecursive. Mounts are listed, not modified.']

def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--list',action='store_true'); ap.add_argument('-a','--all',action='store_true'); ap.add_argument('--result'); ap.add_argument('--recovery',action='store_true'); args=ap.parse_args();
    if args.recovery:
        records=recovery_records()
        print(json.dumps(records,ensure_ascii=True,indent=2) if records else 'No unresolved operations recorded.')
        return
    if os.geteuid()==0 and not args.list: ap.error('Navigator must not run as root')
    p=os.getcwd(); cfg=settings(); hidden=args.all or cfg.get('hidden') is True; order=cfg.get('order','name'); order=order if order in ('name','size','date','type') else 'name'
    if args.list:
        print('\nLocation: '+safe(p))
        rows=tree(p,hidden,order); lim= max(10,shutil.get_terminal_size((80,24)).lines-4)
        for label,_ in rows[:lim]: print(label if sys.stdout.isatty() else SGR.sub('',label))
        if len(rows)>lim: print(f'… {len(rows)-lim} more rows (Ctrl+Down to browse)')
        print(); return
    if not args.result: ap.error('--result is required in navigation mode')
    if not sys.stdin.isatty() or not sys.stdout.isatty(): ap.error('Interactive terminal required')
    try: dest=curses.wrapper(lambda s: Nav(s,p,hidden,order).run())
    except KeyboardInterrupt: return
    with open(args.result,'wb') as f: f.write(os.fsencode(dest)+b'\0')
if __name__=='__main__': main()
