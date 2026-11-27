#!/usr/bin/env bash
set -eu
python3 - <<'PY'
import os, shutil, tempfile, stat
from pathlib import Path
from datetime import datetime
p=Path(os.environ.get('XDG_DATA_HOME') or Path.home()/'.local/share')/'terminal-tree'/'tree.py'
if not p.is_file(): raise SystemExit(f'Terminal Tree not found: {p}')
p=p.resolve(); s=p.read_text(encoding='utf-8')
if '    def mouse_action(self):' in s:
    print('Mouse patch already installed.'); raise SystemExit(0)
def replace(old,new):
    global s
    if s.count(old)!=1: raise SystemExit('Unrecognized Terminal Tree version; no changes made: '+old.splitlines()[0])
    s=s.replace(old,new,1)
anchor='    def go(self,p,record=True,keep=None):'
methods='''    def mouse_event(self):
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

'''
replace(anchor,methods+anchor)
# Enable native mouse reporting rather than VTE's alternate-screen arrow emulation.
replace('        self.s.keypad(True)\n','''        self.s.keypad(True)
        mask = (getattr(curses,'BUTTON1_PRESSED',0) | getattr(curses,'BUTTON3_PRESSED',0)
                | getattr(curses,'BUTTON4_PRESSED',0) | getattr(curses,'BUTTON5_PRESSED',0))
        curses.mouseinterval(0)
        self.mouse_mask = curses.mousemask(mask)[0]
''')
replace('                elif k==curses.KEY_DOWN: self.shift(1)','                elif k==curses.KEY_MOUSE: self.mouse_action()\n                elif k==curses.KEY_DOWN: self.shift(1)')
# Avoid sending mouse reports into text prompts.
replace('            curses.echo(); curses.curs_set(1)','            curses.mousemask(0)\n            curses.echo(); curses.curs_set(1)')
replace('            curses.noecho()\n','            curses.noecho()\n            curses.mousemask(self.mouse_mask)\n')
replace("            if k in (curses.KEY_DOWN,curses.KEY_NPAGE): pos=min(max(0,len(lines)-cap),pos+(cap if k==curses.KEY_NPAGE else 1))", "            if k == curses.KEY_MOUSE: pos=max(0,min(max(0,len(lines)-cap),pos+self.mouse_wheel()))\n            if k in (curses.KEY_DOWN,curses.KEY_NPAGE): pos=min(max(0,len(lines)-cap),pos+(cap if k==curses.KEY_NPAGE else 1))")
if '    def help_page(self):' in s:
    replace("                ('Up / Down', 'Select previous / next item'),", "                ('Up / Down', 'Select previous / next item'),\n                ('Mouse wheel', 'Move one item per wheel event'),\n                ('Left click', 'Open file / enter folder in navigator'),\n                ('Right click', 'Select without opening'),")
    replace('            if k == curses.KEY_DOWN: pos=min(bottom,pos+1)','            if k == curses.KEY_MOUSE: pos=max(0,min(bottom,pos+self.mouse_wheel()))\n            elif k == curses.KEY_DOWN: pos=min(bottom,pos+1)')
else:
    replace("HELP=['Terminal Tree — keys',", "HELP=['Terminal Tree — keys','Mouse wheel: move one item; left click: open; right click: select',")
# Keep previous status text changes, only add mouse hints.
replace("self.msg or 'Arrows navigate |", "self.msg or 'Wheel / click | Arrows navigate |")
compile(s,str(p),'exec')
backup=p.with_name('tree.py.mouse-backup.'+datetime.now().strftime('%Y%m%d-%H%M%S-%f')); shutil.copy2(p,backup)
fd,tmp=tempfile.mkstemp(prefix='.tree-mouse-',dir=p.parent)
try:
    with os.fdopen(fd,'w',encoding='utf-8') as f: f.write(s)
    os.chmod(tmp,stat.S_IMODE(p.stat().st_mode)); os.replace(tmp,p)
finally:
    if os.path.exists(tmp): os.unlink(tmp)
print('Mouse patch installed. Wheel: one item/event. Left click: open. Right click: select.')
print('Backup:',backup)
PY
