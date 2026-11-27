#!/usr/bin/env bash
set -eu
python3 - "$@" <<'PY'
import ast
import os
from pathlib import Path
import shutil
import stat
import tempfile
from datetime import datetime

path = Path(os.environ.get('XDG_DATA_HOME') or Path.home()/'.local/share')/'terminal-tree'/'tree.py'
if not path.is_file():
    raise SystemExit(f'Terminal Tree not found: {path}')
path = path.resolve()
source = path.read_text(encoding='utf-8')
marker = '    def help_page(self):'
if marker in source:
    print('Help patch is already installed.'); raise SystemExit(0)
anchor = '    def go(self,p,record=True,keep=None):'
call = "elif k in ('?',curses.KEY_F1): self.page(HELP)"
if source.count(anchor)!=1 or source.count(call)!=1 or 'def put(self,y,x,t,attr=0):' not in source or 'class Theme:' not in source:
    raise SystemExit('Unrecognized Terminal Tree version. No changes made.')
method = '''    def help_page(self):
        import textwrap
        groups = (
            ('NAVIGATION', (
                ('Up / Down', 'Select previous / next item'),
                ('Left / Right', 'Parent / enter folder'),
                ('Enter', 'Open item and return to shell'),
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
                ('v', 'Paste into current folder'),
                ('F2 / r', 'Rename current item'),
                ('n', 'Create folder'),
                ('t', 'Create empty file'),
            )),
            ('TRASH & SAFETY', (
                ('Delete / d', 'Move selected items to Trash'),
                ('D', 'Permanently delete (confirm)'),
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
            for note in ('Notes: Filter searches this folder only.', 'Drives are listed, not mounted or unmounted here.', 'Copy / cut use the navigator clipboard.'):
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
            if k in ('q','Q','?', '\\x1b','\\n','\\r',curses.KEY_ENTER,curses.KEY_F1): return
            if k == curses.KEY_DOWN: pos=min(bottom,pos+1)
            elif k == curses.KEY_UP: pos=max(0,pos-1)
            elif k == curses.KEY_NPAGE: pos=min(bottom,pos+max(1,cap))
            elif k == curses.KEY_PPAGE: pos=max(0,pos-max(1,cap))
            elif k == curses.KEY_HOME: pos=0
            elif k == curses.KEY_END: pos=bottom

'''
updated = source.replace(anchor,method+anchor,1).replace(call,"elif k in ('?',curses.KEY_F1): self.help_page()",1)
compile(updated,str(path),'exec')
# Only replace the help method and its call site. Preserve any local ESC delay fix.
backup = path.with_name('tree.py.help-backup.'+datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
shutil.copy2(path,backup)
fd,tmp = tempfile.mkstemp(prefix='.tree-help-',dir=path.parent)
try:
    with os.fdopen(fd,'w',encoding='utf-8') as f:
        f.write(updated)
    os.chmod(tmp,stat.S_IMODE(path.stat().st_mode))
    os.replace(tmp,path)
finally:
    if os.path.exists(tmp): os.unlink(tmp)
print('F1 help updated. Open Ctrl+Down, then F1 to try it.')
print('Backup:',backup)
PY
