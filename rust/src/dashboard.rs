//! Responsive application shell. All mutations still dispatch through Workspace.
use crossterm::{
    cursor::MoveTo,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind},
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::{
    io::{self, Write},
    path::PathBuf,
    time::SystemTime,
};
use terminal_tree::{display_name, tree::TreeModel, Entry, KindFilter, Model};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    #[default]
    Tree,
    Grid,
    List,
}

impl View {
    fn name(self) -> &'static str {
        match self { Self::Tree => "Tree", Self::Grid => "Grid", Self::List => "List" }
    }
}

const TOOLBAR: [(&str, &str); 5] = [
    ("Path", "[Path]"), ("Search", "[Search]"), ("Grid", "[Grid]"),
    ("List", "[List]"), ("Tree", "[Tree]"),
];

fn toolbar_start(width: u16) -> Option<u16> {
    let length = TOOLBAR.iter().map(|(_, s)| s.len() + 1).sum::<usize>() as u16;
    (width >= length + 10).then(|| width - length)
}

fn toolbar_hit(width: u16, column: u16) -> Option<usize> {
    let mut x = toolbar_start(width)?;
    for (i, (_, label)) in TOOLBAR.iter().enumerate() {
        let end = x + label.len() as u16;
        if column >= x && column < end { return Some(i); }
        x = end + 1;
    }
    None
}

#[derive(Clone, Copy)]
struct Cells {
    columns: usize,
    width: u16,
    stride: usize,
    groups: usize,
}

impl Cells {
    fn new(width: u16, height: usize, view: View, zoom: usize) -> Self {
        let stride = if view == View::Grid { (zoom + 2).min(height.max(1)) } else { zoom.max(1).min(height.max(1)) };
        let columns = if view == View::Grid { (usize::from(width) / (12 + zoom * 5)).max(1) } else { 1 };
        Self { columns, width: width / columns as u16, stride, groups: (height / stride).max(1) }
    }
    fn capacity(self) -> usize { self.columns * self.groups }
    fn hit(self, x: u16, y: usize) -> Option<usize> {
        if self.width == 0 { return None; }
        let col = usize::from(x / self.width);
        let row = y / self.stride;
        (col < self.columns && row < self.groups).then_some(row * self.columns + col)
    }
}

fn draw_toolbar(out: &mut impl Write, width: u16, view: View, prefs: &super::app_settings::Preferences) -> io::Result<u16> {
    let Some(start) = toolbar_start(width) else { return Ok(width); };
    let mut x = start;
    for (name, label) in TOOLBAR {
        queue!(out, MoveTo(x, 1))?;
        if name == view.name() { queue!(out, SetBackgroundColor(prefs.selection), SetForegroundColor(prefs.accent))?; }
        queue!(out, Print(label), SetForegroundColor(prefs.foreground), SetBackgroundColor(prefs.background))?;
        x += label.len() as u16 + 1;
    }
    Ok(start.saturating_sub(1))
}

fn breadcrumb(path: &std::path::Path) -> String {
    path.components().map(|p| display_name(p.as_os_str())).collect::<Vec<_>>().join(" > ")
}

fn breadcrumb_hit(path:&std::path::Path,column:u16,width:u16)->Option<PathBuf> {
    use unicode_width::UnicodeWidthStr;
    let mut prefix=PathBuf::new();
    let mut x=0usize;
    for part in path.components(){
        prefix.push(part.as_os_str());
        let label=display_name(part.as_os_str());
        let end=x+label.width();
        if end>usize::from(width){break;}
        if (x..end).contains(&usize::from(column)){return Some(prefix);}
        x=end+3;
    }
    None
}
fn list_sort_hit(column:u16,width:u16,details:bool)->terminal_tree::Order {
    use terminal_tree::Order;
    if details&&width>=48 {
        if column>=width-11 {Order::Type}
        else if column>=width-22 {Order::Size}
        else{Order::Name}
    }else if width>=30&&column>=width-12 {Order::Size}
    else{Order::Name}
}
fn list_heading(order:terminal_tree::Order,focused:bool,width:u16,details:bool)->String{
    use terminal_tree::Order;
    let name=if focused{"Files * / Name"}else{"Files / Name"};
    list_columns(&format!("{name}{}",if order==Order::Name{" ↑"}else{""}),
        if order==Order::Size{"Size ↑"}else{"Size"},
        if order==Order::Type{"Type ↑"}else{"Type"},width,details)
}
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]
enum PreviewStyle {#[default] Plain,Heading,Code,Quote}
#[cfg(feature = "native-ops")]
fn markdown_preview(lines:&[String],width:usize)->(Vec<String>,Vec<PreviewStyle>){
    use unicode_width::UnicodeWidthChar;
    let mut out=Vec::new();
    let mut styles=Vec::new();
    let mut fence=false;
    let width=width.max(1);
    for (i,raw) in lines.iter().enumerate(){
        if out.len()>=8192{break;}
        let trimmed=raw.trim_start();
        let (text,style)=if i==0 {(raw.clone(),PreviewStyle::Heading)}
        else if trimmed.starts_with(&char::from(96).to_string().repeat(3))||trimmed.starts_with("~~~"){
            fence=!fence;(raw.clone(),PreviewStyle::Code)
        }else if fence{(raw.clone(),PreviewStyle::Code)}
        else if let Some((_,title))=trimmed.split_once(' ').filter(|(p,_)|!p.is_empty()&&p.len()<=6&&p.bytes().all(|c|c==b'#')){
            (title.to_owned(),PreviewStyle::Heading)
        }else if let Some(s)=trimmed.strip_prefix("> "){(format!("│ {s}"),PreviewStyle::Quote)}
        else if let Some(s)=trimmed.strip_prefix("- ").or_else(||trimmed.strip_prefix("* ")){(format!("• {s}"),PreviewStyle::Plain)}
        else{(raw.clone(),PreviewStyle::Plain)};
        let mut line=String::new();
        let mut cells=0;
        for ch in text.chars(){
            let n=ch.width().unwrap_or(0);
            if n>width{continue;}
            if cells+n>width{
                if out.len()>=8192{return (out,styles);}
                out.push(std::mem::take(&mut line));styles.push(style);cells=0;
            }
            line.push(ch);cells+=n;
        }
        if out.len()<8192{out.push(line);styles.push(style);}
    }
    (out,styles)
}
fn human_size(size:u64)->String {
    if size<1024 {return format!("{size} B");}
    let mut n=size as f64;
    for unit in ["KiB","MiB","GiB","TiB","PiB","EiB"] {
        n/=1024.0;
        if n<1024.0 || unit=="EiB" {return format!("{n:.1} {unit}");}
    }
    unreachable!()
}
fn entry_type(entry:&Entry)->String {
    if entry.symlink {"Link".into()} else if entry.directory {"Folder".into()} else {
        entry.path.extension().map(display_name).filter(|s|!s.is_empty()).unwrap_or_else(||"File".into())
    }
}
fn pad_cell(text:&str,width:usize)->String {
    use unicode_width::UnicodeWidthStr;
    let value=super::clip(text,width);
    let pad=width.saturating_sub(value.width());
    format!("{value}{}"," ".repeat(pad))
}
fn list_columns(name:&str, size:&str, kind:&str,width:u16,details:bool)->String {
    let w=usize::from(width);
    if details&&w>=48 {
        format!("{} {:>10} {}",pad_cell(name,w-23),size,pad_cell(kind,11))
    } else if w>=30 {
        format!("{} {:>12}",pad_cell(name,w-13),size)
    } else {super::clip(name,w)}
}
fn row_text(entry: &Entry, index: usize, count: usize, view: View, glyph: &str, marked: bool, width: u16,details:bool) -> String {
    let mark = if marked { "[x]" } else { "   " };
    match view {
        View::Tree => format!("{} {mark} {glyph} {}", if index + 1 == count { "└──" } else { "├──" }, entry.name),
        View::List => list_columns(&format!("{mark} {glyph} {}",entry.name),&if entry.directory{"<DIR>".into()}else{human_size(entry.size)},&entry_type(entry),width,details),
        _ => format!("{mark} {glyph} {}", entry.name),
    }
}

const MENUS: [(&str, u16); 7] = [
    ("File", 15), ("Edit", 21), ("View", 27), ("Go", 33), ("Tools", 37), ("Settings", 44), ("Add-ons", 54),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    width: u16,
    height: u16,
    sidebar: u16,
    preview: u16,
}

impl Layout {
    fn new(width: u16, height: u16, preview: bool) -> Self {
        let width = width.saturating_sub(1);
        let sidebar = if width >= 54 { 19.min(width / 4) } else { 0 };
        let preview = if preview && width >= 92 { width / 3 } else { 0 };
        Self { width, height, sidebar, preview }
    }

    fn first(&self) -> u16 { if self.sidebar > 0 { self.sidebar + 1 } else { 0 } }
    fn end(&self) -> u16 {
        if self.preview > 0 { self.width - self.preview - 1 } else { self.width }
    }
    fn capacity(&self) -> usize { usize::from(self.height.saturating_sub(7)).max(1) }
    fn file_row(&self, column: u16, row: u16) -> Option<u16> {
        if self.height >= 8 && row >= 4 && row < self.height - 3
            && column >= self.first() && column < self.end()
        {
            Some(row - 3)
        } else { None }
    }
}

const NAV_KEYS: [KeyCode;9]=[KeyCode::Up,KeyCode::Down,KeyCode::PageUp,KeyCode::PageDown,KeyCode::Home,KeyCode::End,KeyCode::Right,KeyCode::Left,KeyCode::F(5)];
const NAV_LABELS: [&str;9]=["Up","Down","Page up","Page down","First","Last","Expand / right","Collapse / left","Refresh"];
#[derive(Clone,Copy)]
enum Divider { Sidebar, Preview, Dual, Terminal }

struct Tab {
    model: Model,
    other: Option<Model>,
    dual: bool,
    active_right: bool,
}

fn percent(width:u16, ratio:u16)->u16 { (u32::from(width)*u32::from(ratio)/100) as u16 }
fn dual_split(width:u16, ratio:u16)->u16 { percent(width,ratio).clamp(16,width.saturating_sub(17).max(16)) }

type Place = (String, Option<PathBuf>);
type PreviewKey = (PathBuf, u64, Option<SystemTime>);

pub struct State {
    places: Vec<Place>,
    bookmarks: Vec<PathBuf>,
    trash: Option<PathBuf>,
    side_focus: bool,
    side_cursor: usize,
    side_top: usize,
    preview_enabled: bool,
    frame_cache: super::dashboard_frame::FrameCache,
    preview_key: Option<PreviewKey>,
    preview_lines: Vec<String>,
    preview_scroll: usize,
    preview_styles: Vec<PreviewStyle>,
    preview_mode: u8,
    #[cfg(feature = "native-ops")]
    visual_frame: Option<terminal_tree::visual_preview::Frame>,
    #[cfg(feature = "native-ops")]
    visual_bitmap: bool,
    #[cfg(feature = "native-ops")]
    graphics_slot: super::native_graphics::Slot,
    #[cfg(feature = "native-ops")]
    graphics_placed: bool,
    #[cfg(feature = "native-ops")]
    visual_size: (u32,u32),
    #[cfg(feature = "native-ops")]
    visual_page: u32,
    dual: bool,
    active_right: bool,
    other: Option<Model>,
    pane_switched: bool,
    view: View,
    zoom: usize,
    location: Option<String>,
    expanded_tree: Option<TreeModel>,
    tabs: Vec<Option<Tab>>,
    active_tab: usize,
    dragging: Option<Divider>,
    #[cfg(feature = "native-ops")]
    terminal_panel: super::terminal_panel::Panel,
    icons: super::file_icons::Style,
    prefs: super::app_settings::Preferences,
    #[cfg(feature = "native-ops")]
    store: Option<super::settings::Store>,
    #[cfg(feature = "native-ops")]
    last_tool: Option<String>,
    #[cfg(feature = "native-ops")]
    last_saved: Option<serde_json::Value>,
    #[cfg(feature = "native-ops")]
    disk_cache: Option<(PathBuf,std::time::Instant,String)>,
}

impl State {
    pub fn new(trash: Option<PathBuf>) -> Self {
        let mut state = Self {
            places: Vec::new(), bookmarks: Vec::new(), trash,
            side_focus: false, side_cursor: 0, side_top: 0, preview_enabled: true,
            frame_cache: Default::default(),
            preview_key: None, preview_lines: Vec::new(), preview_scroll: 0, preview_styles: Vec::new(), preview_mode: 0,
            #[cfg(feature = "native-ops")]
            visual_frame: None,
            #[cfg(feature = "native-ops")]
            visual_bitmap: false,
            #[cfg(feature = "native-ops")]
            graphics_slot: super::native_graphics::Slot::new(1),
            #[cfg(feature = "native-ops")]
            graphics_placed: false,
            #[cfg(feature = "native-ops")]
            visual_size: (0,0),
            #[cfg(feature = "native-ops")]
            visual_page: 1,
            dual: false, active_right: false, other: None, pane_switched: false,
            view: View::Tree, zoom: 1, location: None, expanded_tree: None,
            tabs: vec![None], active_tab: 0, dragging: None,
            #[cfg(feature = "native-ops")]
            terminal_panel: Default::default(),
            icons: super::file_icons::Style::from_environment(),
            prefs: Default::default(),
            #[cfg(feature = "native-ops")]
            store: None,
            #[cfg(feature = "native-ops")]
            last_tool: None,
            #[cfg(feature = "native-ops")]
            last_saved: None,
            #[cfg(feature = "native-ops")]
            disk_cache: None,
        };
        state.refresh_places();
        state
    }

    #[cfg(feature = "native-ops")]
    fn config_path() -> Option<PathBuf> {
        std::env::var_os("TT_APP_CONFIG_DIR").map(PathBuf::from).or_else(|| {
            super::platform::home_dir().map(|home| {
                std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
                    .filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".config"))
                    .join("terminal-tree")
            })
        })
    }

    #[cfg(feature = "native-ops")]
    pub fn load_preferences(&mut self, model: &mut Model) -> io::Result<()> {
        let Some(path)=Self::config_path() else { return Ok(()); };
        let store=match super::settings::Store::new(&path) {
            Ok(store)=>store,
            Err(error) if error.kind()==io::ErrorKind::NotFound=>return Ok(()),
            Err(error)=>return Err(error),
        };
        if let Some(value) = store.load_app()? {
            let (prefs,icons,preview,view,zoom) = super::app_settings::Preferences::decode(&value)?;
            let bookmarks=super::app_settings::decode_bookmarks(&value)?;
            let hidden=value.get("hidden").map(|v| v.as_bool().ok_or_else(||io::Error::other("invalid hidden setting"))).transpose()?.unwrap_or(model.hidden);
            let order=match value.get("order").and_then(|v|v.as_str()) {
                None=>model.order,Some("Name")=>terminal_tree::Order::Name,Some("Size")=>terminal_tree::Order::Size,
                Some("Date")=>terminal_tree::Order::Date,Some("Type")=>terminal_tree::Order::Type,
                _=>return Err(io::Error::other("invalid sort setting")),
            };
            model.set_view(hidden,order,model.kind_filter,model.query.clone())?;
            self.prefs = prefs;
            if std::env::var_os("TT_ICON_STYLE").is_none() { self.icons = icons; }
            self.preview_enabled = preview;
            self.view = match view.as_str() { "Grid" => View::Grid, "List" => View::List, _ => View::Tree };
            self.zoom = zoom;
            self.bookmarks=bookmarks;
            self.refresh_places();
            self.last_saved=Some(self.preference_value(model)?);
        }
        self.store = Some(store);
        Ok(())
    }

    #[cfg(feature = "native-ops")]
    fn preference_value(&self, model: &Model) -> io::Result<serde_json::Value> {
        let mut v=self.prefs.encode(self.icons,self.preview_enabled,self.view.name(),self.zoom);
        v["bookmarks"]=super::app_settings::encode_bookmarks(&self.bookmarks)?;
        v["hidden"]=serde_json::json!(model.hidden);
        v["order"]=serde_json::json!(format!("{:?}",model.order));
        Ok(v)
    }

    #[cfg(feature = "native-ops")]
    fn save_preferences(&mut self, model: &Model) -> io::Result<()> {
        if self.store.is_none() {
            let path=Self::config_path().ok_or_else(||io::Error::other("configuration location unavailable"))?;
            if !path.is_absolute() { return Err(io::Error::other("configuration directory must be absolute")); }
            let label=format!("Create or use {}",display_name(path.as_os_str()));
            if super::choose("Save application preferences in this private directory?", &["Cancel".into(),label])?!=Some(1) {
                return Ok(());
            }
            let store=match super::settings::Store::new(&path) {
                Ok(store)=>store,
                Err(error) if error.kind()==io::ErrorKind::NotFound=>super::settings::Store::create_app_directory(&path)?,
                Err(error)=>return Err(error),
            };
            if store.load_app()?.is_some() {
                return Err(io::Error::other("existing preferences found; restart to load before saving"));
            }
            self.store=Some(store);
        }
        let value=self.preference_value(model)?;
        if let Some(store)=&self.store {
            store.save_app(&value)?;
            self.last_saved=Some(value);
            super::page("Preferences saved",&["Application preferences saved.".into()])?;
        }
        Ok(())
    }

    #[cfg(feature = "native-ops")]
    pub fn save_on_exit(&mut self, model: &Model) -> io::Result<()> {
        if !self.prefs.autosave { return Ok(()); }
        if let Some(store)=&self.store {
            let value=self.preference_value(model)?;
            if self.last_saved.as_ref()!=Some(&value) {
                store.save_app(&value)?;
                self.last_saved=Some(value);
            }
        }
        Ok(())
    }

    fn panel_rows(&self,h:u16)->u16 {
        if cfg!(feature = "native-ops") && self.prefs.terminal_visible && h>=16 {percent(h,self.prefs.terminal_percent).clamp(5,h-9)} else {0}
    }
    #[cfg(feature = "native-ops")]
    pub fn terminal_running(&self)->bool {self.terminal_panel.running()}
    #[cfg(feature = "native-ops")]
    pub fn tick_terminal(&mut self)->io::Result<bool> {
        let finished=self.terminal_panel.tick()?;
        if finished {self.expanded_tree=None;self.preview_key=None;}
        Ok(finished)
    }
    #[cfg(feature = "native-ops")]
    fn draw_terminal(&mut self,out:&mut impl Write,h:u16,width:u16,model:&Model)->io::Result<()> {
        let rows=self.panel_rows(h);
        if rows>0 {self.terminal_panel.resize_shell(width,rows.saturating_sub(3))?;self.terminal_panel.draw(out,h-rows,rows,width,&model.directory,&self.prefs)?;}
        Ok(())
    }
    #[cfg(feature = "native-ops")]
    fn terminal_button(&self,out:&mut impl Write,width:u16)->io::Result<()> {
        const X:u16=63;
        let label=if self.terminal_panel.running() {"[Terminal*]"}else if self.prefs.terminal_visible {"[Terminal -]"}else{"[Terminal +]"};
        if width>=X+label.len() as u16 {
            queue!(out,MoveTo(X,0),SetForegroundColor(self.prefs.accent),Print(label),SetForegroundColor(self.prefs.foreground))?;
        }
        if width>=83 {queue!(out,MoveTo(78,0),SetForegroundColor(self.prefs.accent),Print(if self.prefs.companions[2]{"[Git]"}else{"[git]"}),SetForegroundColor(self.prefs.foreground))?;}
        Ok(())
    }
    fn disk_status(&mut self,path:&std::path::Path)->String{
        #[cfg(feature = "native-ops")]
        {
            if let Some((cached,when,label))=&self.disk_cache{
                if cached==path&&when.elapsed()<std::time::Duration::from_secs(2){return label.clone();}
            }
            let label=match rustix::fs::statvfs(path) {
                Ok(s)=>format!("Free: {}",human_size((s.f_bavail as u64).saturating_mul(s.f_frsize as u64))),
                Err(_)=>"Free: unavailable".into(),
            };
            self.disk_cache=Some((path.to_path_buf(),std::time::Instant::now(),label.clone()));
            label
        }
        #[cfg(not(feature = "native-ops"))]
        {let _=path;String::new()}
    }
    fn task_label(&self)->&'static str {
        #[cfg(feature = "native-ops")]
        if self.terminal_panel.interactive(){return "PTY active";}
        #[cfg(feature = "native-ops")]
        if self.terminal_panel.running(){return "Bash running";}
        "idle"
    }
    fn layout(&self, w: u16, h: u16) -> Layout {
        let mut layout = Layout::new(w,h,self.preview_enabled);
        if !self.prefs.sidebar { layout.sidebar = 0; }
        else if layout.sidebar > 0 { layout.sidebar=self.prefs.sidebar_width.min(layout.width/3); }
        if layout.preview>0 { layout.preview=percent(layout.width,self.prefs.preview_percent); }
        layout
    }

    fn key_settings(&mut self)->io::Result<()> {
        loop {
            let mut labels:Vec<_>=NAV_LABELS.iter().enumerate().map(|(i,name)|format!("{name}: {:?}",NAV_KEYS[self.prefs.navigation[i]])).collect();
            labels.push("Reset navigation shortcuts".into());
            let Some(action)=super::choose("Settings / Navigation shortcuts",&labels)? else {return Ok(());};
            if action==9 {self.prefs.navigation=[0,1,2,3,4,5,6,7,8];continue;}
            let keys:Vec<_>=NAV_KEYS.iter().map(|key|format!("{key:?}")).collect();
            if let Some(key)=super::choose("Select key (swaps existing navigation binding)",&keys)? {
                if let Some(other)=self.prefs.navigation.iter().position(|&n|n==key) {
                    self.prefs.navigation.swap(action,other);
                }
            }
        }
    }

    pub fn remap_input(&self,mut event:Event,searching:bool)->Event {
        #[cfg(feature = "native-ops")]
        if self.terminal_panel.focused {return event;}
        if !searching && self.location.is_none() {
            if let Event::Key(key)=&mut event {
                if key.modifiers==KeyModifiers::NONE {
                    if let Some(index)=NAV_KEYS.iter().position(|&code|code==key.code) {
                        if let Some(action)=self.prefs.navigation.iter().position(|&n|n==index) {
                            key.code=NAV_KEYS[action];
                        }
                    }
                }
            }
        }
        event
    }

    fn tab_slots(&self,width:u16)->(usize,u16,usize) {
        // Keep the active tab visible even on a compact terminal.
        let cell=18u16.min(width.saturating_sub(4).max(4));
        let count=usize::from(width.saturating_sub(4)/cell).max(1).min(self.tabs.len());
        let first=self.active_tab.saturating_sub(count-1).min(self.tabs.len()-count);
        (first,cell,count)
    }

    fn draw_tabs(&self,out:&mut impl Write,model:&Model,width:u16)->io::Result<()> {
        if width<8 {return Ok(());}
        let (first,cell,count)=self.tab_slots(width);
        for (slot,index) in (first..first+count).enumerate() {
            let path=if index==self.active_tab { &model.directory } else if let Some(tab)=&self.tabs[index] { &tab.model.directory } else {continue};
            let name=path.file_name().map(display_name).unwrap_or_else(||display_name(path.as_os_str()));
            let label=format!("[{} {}]",index+1,name);
            queue!(out,MoveTo(slot as u16*cell,2))?;
            if index==self.active_tab {queue!(out,SetBackgroundColor(self.prefs.selection),SetForegroundColor(self.prefs.accent))?;}
            queue!(out,Print(super::clip(&label,usize::from(cell.saturating_sub(1)))),SetForegroundColor(self.prefs.foreground),SetBackgroundColor(self.prefs.background))?;
        }
        if count as u16*cell+3<=width {
            queue!(out,MoveTo(count as u16*cell,2),Print("[+]"))?;
        }
        Ok(())
    }

    fn clear_tab_transients(&mut self) {
        self.side_focus=false;
        self.expanded_tree=None;
        self.location=None;
        self.preview_key=None;
        self.dragging=None;
        self.pane_switched=true;
    }

    fn park(&mut self,model:&mut Model,mut incoming:Tab)->Tab {
        std::mem::swap(model,&mut incoming.model);
        std::mem::swap(&mut self.other,&mut incoming.other);
        std::mem::swap(&mut self.dual,&mut incoming.dual);
        std::mem::swap(&mut self.active_right,&mut incoming.active_right);
        self.clear_tab_transients();
        incoming
    }

    fn new_tab(&mut self,model:&mut Model)->io::Result<()> {
        if self.tabs.len()>=8 {return Err(io::Error::other("maximum eight tabs"));}
        let mut next=Model::new(model.directory.clone())?;
        next.set_view(model.hidden,model.order,model.kind_filter,String::new())?;
        let parked=self.park(model,Tab{model:next,other:None,dual:false,active_right:false});
        self.tabs[self.active_tab]=Some(parked);
        self.tabs.push(None);
        self.active_tab=self.tabs.len()-1;
        Ok(())
    }

    fn switch_tab(&mut self,model:&mut Model,index:usize)->io::Result<()> {
        if index==self.active_tab || index>=self.tabs.len() {return Ok(());}
        let tab=self.tabs[index].as_mut().ok_or_else(||io::Error::other("tab unavailable"))?;
        tab.model.reload()?;
        let incoming=self.tabs[index].take().ok_or_else(||io::Error::other("tab unavailable"))?;
        let parked=self.park(model,incoming);
        self.tabs[self.active_tab]=Some(parked);
        self.active_tab=index;
        Ok(())
    }

    fn close_tab(&mut self,model:&mut Model)->io::Result<()> {
        if self.tabs.len()==1 {return Ok(());}
        let previous=self.active_tab;
        let next=if previous>0 {previous-1}else{1};
        self.switch_tab(model,next)?;
        self.tabs.remove(previous);
        if previous<self.active_tab {self.active_tab-=1;}
        Ok(())
    }

    fn resize_drag(&mut self,divider:Divider,x:u16,width:u16,y:u16,height:u16) {
        let ratio=if width==0 {50}else{(u32::from(x.min(width))*100/u32::from(width)) as u16};
        match divider {
            Divider::Sidebar=>self.prefs.sidebar_width=x.clamp(12,48),
            Divider::Preview=>self.prefs.preview_percent=(100-ratio).clamp(18,45),
            Divider::Dual=>self.prefs.dual_percent=ratio.clamp(25,75),
            Divider::Terminal=>if height>0 {self.prefs.terminal_percent=(u32::from(height.saturating_sub(y))*100/u32::from(height)).clamp(20,60) as u16;},
        }
    }

    fn appearance(&mut self) -> io::Result<()> {
        loop {
            let labels = [
                "Terminal defaults".into(), "Dark theme".into(), "Light theme".into(),
                "Custom text color (#RRGGBB)".into(), "Custom background color".into(),
                "Custom accent color".into(), "Custom selection color".into(),
                "Font family and size help".into(),
            ];
            match super::choose("Settings / Appearance", &labels)? {
                None => return Ok(()),
                Some(0) => {
                    let defaults = super::app_settings::Preferences::default();
                    (self.prefs.foreground,self.prefs.background,self.prefs.accent,self.prefs.selection) =
                        (defaults.foreground,defaults.background,defaults.accent,defaults.selection);
                }
                Some(1) => {
                    self.prefs.foreground=Color::Rgb{r:230,g:230,b:230};
                    self.prefs.background=Color::Rgb{r:36,g:36,b:36};
                    self.prefs.accent=Color::Rgb{r:100,g:190,b:245};
                    self.prefs.selection=Color::Rgb{r:60,g:85,b:110};
                }
                Some(2) => {
                    self.prefs.foreground=Color::Rgb{r:25,g:25,b:25};
                    self.prefs.background=Color::Rgb{r:245,g:245,b:245};
                    self.prefs.accent=Color::Rgb{r:0,g:80,b:160};
                    self.prefs.selection=Color::Rgb{r:190,g:215,b:240};
                }
                Some(i @ 3..=6) => {
                    #[cfg(feature = "native-ops")]
                    if let Some(value)=super::workspace::prompt("Color (#RRGGBB or default):")? {
                        match super::app_settings::parse_color(&value) {
                            Ok(color)=>match i {3=>self.prefs.foreground=color,4=>self.prefs.background=color,5=>self.prefs.accent=color,_=>self.prefs.selection=color},
                            Err(error)=>super::page("Invalid color",&[error.to_string()])?,
                        }
                    }
                    #[cfg(not(feature = "native-ops"))]
                    { let _ = i; super::page("Appearance",&["Custom RGB entry requires native Linux".into()])?; }
                }
                _ => super::page("Terminal font", &[
                    "Font family and actual point size are controlled by your terminal emulator.".into(),
                    "For Seti glyphs, select a compatible Nerd Font Mono in the terminal.".into(),
                    "Ctrl+mouse wheel changes the application layout density, not the font.".into(),
                ])?,
            }
        }
    }

    fn display_settings(&mut self, model: &mut Model) -> io::Result<()> {
        loop {
            let labels = [
                format!("Sidebar: {}",self.prefs.sidebar),
                format!("Live preview: {}",self.preview_enabled),
                format!("Location toolbar: {}",self.prefs.toolbar),
                format!("File icons: {}",self.prefs.show_icons),
                format!("File-type colors: {}",self.prefs.file_colors),
                format!("Extra status details: {}",self.prefs.status_details),
                format!("Hidden files: {}",model.hidden),
                format!("Layout zoom: {}",self.zoom),
                format!("File view: {}",self.view.name()),
                "Reset panel sizes".into(),
                format!("Terminal panel: {}",self.prefs.terminal_visible),
                format!("List type column: {}",self.prefs.list_details),
            ];
            match super::choose("Settings / Display (Enter toggles)",&labels)? {
                None=>return Ok(()),
                Some(0)=>{self.prefs.sidebar=!self.prefs.sidebar; self.side_focus=false;},
                Some(1)=>{self.preview_enabled=!self.preview_enabled; self.preview_key=None;},
                Some(2)=>self.prefs.toolbar=!self.prefs.toolbar,
                Some(3)=>self.prefs.show_icons=!self.prefs.show_icons,
                Some(4)=>self.prefs.file_colors=!self.prefs.file_colors,
                Some(5)=>self.prefs.status_details=!self.prefs.status_details,
                Some(6)=>{model.set_view(!model.hidden,model.order,model.kind_filter,model.query.clone())?; self.pane_switched=true; self.expanded_tree=None;},
                Some(7)=>self.zoom=self.zoom%3+1,
                Some(8)=>self.choose_view(match self.view {View::Tree=>View::Grid,View::Grid=>View::List,View::List=>View::Tree}),
                Some(9)=>{self.prefs.sidebar_width=19;self.prefs.preview_percent=33;self.prefs.dual_percent=50;self.prefs.terminal_percent=28;},
                Some(10)=>{self.prefs.terminal_visible=!self.prefs.terminal_visible; #[cfg(feature = "native-ops")] {self.terminal_panel.focused=false;}},
                _=>self.prefs.list_details=!self.prefs.list_details,
            }
        }
    }

    fn icon_settings(&mut self) -> io::Result<()> {
        loop {
            let labels = [
                format!("Icon style: {}",self.icons.name()),
                format!("Show icons: {}",self.prefs.show_icons),
                format!("Custom extension mappings: {} (import JSON)",self.prefs.custom.len()),
                "Remove custom mappings".into(),
            ];
            match super::choose("Settings / Icons",&labels)? {
                None=>return Ok(()),
                Some(0)=>self.choose_icons()?,
                Some(1)=>self.prefs.show_icons=!self.prefs.show_icons,
                Some(2)=>{
                    #[cfg(feature = "native-ops")]
                    if let Some(path)=super::workspace::prompt("Absolute icon JSON path:")? {
                        let result=terminal_tree::read_icon_pack(std::path::Path::new(&path))
                            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(io::Error::other))
                            .and_then(|v| super::app_settings::parse_pack(&v));
                        match result {
                            Ok(map)=>self.prefs.custom=map,
                            Err(error)=>super::page("Icon import refused",&[terminal_tree::display_name(std::ffi::OsStr::new(&error.to_string()))])?,
                        }
                    }
                    #[cfg(not(feature = "native-ops"))]
                    super::page("Icon import",&["Native Linux required".into()])?;
                }
                _=>self.prefs.custom.clear(),
            }
        }
    }

    fn integrations(&mut self) -> io::Result<()> {
        #[cfg(feature = "native-ops")]
        {
            super::addons_ui::show(&mut self.prefs,self.last_tool.as_deref())?;
            self.preview_key=None;self.preview_lines.clear();self.visual_frame=None;
            self.frame_cache.invalidate();
            Ok(())
        }
        #[cfg(not(feature = "native-ops"))]
        super::page("Add-ons",&["Companion integrations require native Linux.".into()])
    }

    fn settings_center(&mut self, model: &mut Model) -> io::Result<()> {
        loop {
            let labels = [
                "Appearance / text and colors".into(),
                "Icons / style and import".into(),
                "Display / panels and view".into(),
                "Add-ons / integration status".into(),
                "Save application preferences".into(),
                format!("Autosave on normal exit: {}",self.prefs.autosave),
                "Manage bookmarks".into(),
                "Navigation shortcuts".into(),
                "Reset application preferences (session)".into(),
            ];
            match super::choose("Settings Center",&labels)? {
                None=>return Ok(()),
                Some(0)=>self.appearance()?,
                Some(1)=>self.icon_settings()?,
                Some(2)=>self.display_settings(model)?,
                Some(3)=>self.integrations()?,
                Some(4)=>{
                    #[cfg(feature = "native-ops")]
                    self.save_preferences(model)?;
                    #[cfg(not(feature = "native-ops"))]
                    super::page("Save preferences",&["Session only on portable builds.".into()])?;
                }
                Some(5)=>{
                    #[cfg(feature = "native-ops")]
                    {
                        if self.store.is_none() {
                            super::page("Autosave",&["Save application preferences once to establish the private configuration directory.".into()])?;
                        } else {
                            self.prefs.autosave=!self.prefs.autosave;
                            // Persist the explicit opt-in/out now, including when turning autosave off.
                            self.save_preferences(model)?;
                        }
                    }
                    #[cfg(not(feature = "native-ops"))]
                    super::page("Autosave",&["Session only on portable builds.".into()])?;
                }
                Some(6)=>self.manage_bookmarks()?,
                Some(7)=>self.key_settings()?,
                _=>{
                    let labels=["Cancel".into(),"Reset all application display and companion options".into()];
                    if super::choose("Reset preferences?",&labels)?==Some(1) {
                        self.prefs=Default::default(); self.icons=Default::default();
                        self.preview_enabled=true; self.zoom=1; self.choose_view(View::Tree);
                    }
                }
            }
        }
    }

    fn manage_bookmarks(&mut self) -> io::Result<()> {
        loop {
            let labels:Vec<_>=self.bookmarks.iter().map(|p|display_name(p.as_os_str())).collect();
            if labels.is_empty() {
                return super::page("Manage bookmarks",&["No bookmarks. Press b in a directory to add it.".into()]);
            }
            let Some(index)=super::choose("Manage bookmarks",&labels)? else { return Ok(()); };
            let actions=["Move up".into(),"Move down".into(),"Remove bookmark".into(),"Cancel".into()];
            match super::choose("Bookmark actions",&actions)? {
                Some(0) if index>0=>self.bookmarks.swap(index,index-1),
                Some(1) if index+1<self.bookmarks.len()=>self.bookmarks.swap(index,index+1),
                Some(2)=>{self.bookmarks.remove(index);},
                _=>(),
            }
            self.refresh_places();
        }
    }

    fn refresh_places(&mut self) {
        let mut places = vec![("PLACES".into(), None)];
        if let Some(home) = super::platform::home_dir() {
            places.push(("Home".into(), Some(home.clone())));
            for name in ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos"] {
                let p = home.join(name);
                if p.is_dir() { places.push((name.into(), Some(p))); }
            }
            let trash = self.trash.clone().unwrap_or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| home.join(".local/share"))
                    .join("Trash/files")
            });
            if trash.is_dir() { places.push(("Trash".into(), Some(trash))); }
        }
        places.push(("DEVICES".into(), None));
        if let Ok(root) = super::platform::filesystem_root() {
            places.push(("File System".into(), Some(root)));
        }
        #[cfg(target_os = "linux")]
        if let Ok(paths) = std::fs::read("/proc/self/mountinfo")
            .and_then(|bytes| super::places::media_mounts(&bytes))
        {
            for p in paths.into_iter().take(128) {
                let label = p.file_name().map(display_name)
                    .unwrap_or_else(|| display_name(p.as_os_str()));
                places.push((label, Some(p)));
            }
        }
        places.push(("BOOKMARKS".into(), None));
        for p in &self.bookmarks {
            places.push((p.file_name().map(display_name)
                .unwrap_or_else(|| display_name(p.as_os_str())), Some(p.clone())));
        }
        self.places = places;
        self.side_cursor = self.side_cursor.min(self.places.len().saturating_sub(1));
        if self.places[self.side_cursor].1.is_none() {
            self.side_cursor = self.places.iter().position(|p| p.1.is_some()).unwrap_or(0);
        }
    }

    fn move_side(&mut self, forward: bool) {
        let next = if forward {
            (self.side_cursor + 1..self.places.len()).find(|&i| self.places[i].1.is_some())
        } else {
            (0..self.side_cursor).rev().find(|&i| self.places[i].1.is_some())
        };
        if let Some(i) = next { self.side_cursor = i; }
    }

    fn open_side(&mut self, model: &mut Model) -> io::Result<()> {
        if let Some(path) = self.places.get(self.side_cursor).and_then(|p| p.1.clone()) {
            model.go(path)?;
            self.side_focus = false;
            self.preview_key = None;
        }
        Ok(())
    }

    fn preview(&mut self, entry: Option<&Entry>, width:u16, rows:usize) {
        let key = entry.map(|e| (e.path.clone(), e.size, e.modified));
        if key!=self.preview_key {self.preview_scroll=0;}
        #[cfg(feature = "native-ops")]
        {
            let size=if super::native_graphics::supported(){super::native_graphics::pixel_bounds(width,rows.saturating_sub(2).min(u16::MAX as usize) as u16)}else{(u32::from(width).clamp(1,160),(rows.saturating_sub(2)*2).clamp(1,160) as u32)};
            if key != self.preview_key {self.visual_page=1;}
            if size!=self.visual_size {self.visual_size=size;self.preview_lines.clear();self.preview_scroll=0;}
        }
        #[cfg(not(feature = "native-ops"))]
        let _=(width,rows);
        if key == self.preview_key && !self.preview_lines.is_empty() { return; }
        self.preview_key = key;
        self.preview_styles.clear();
        #[cfg(feature = "native-ops")]
        {
            self.visual_frame=None;self.visual_bitmap=false;self.graphics_placed=false;
            if let Some(e)=entry.filter(|e|self.preview_mode==0&&!e.directory&&!e.symlink&&terminal_tree::visual_preview::format(&e.path).is_some()) {
                if !self.prefs.visual_preview || !terminal_tree::visual_preview::installed() {
                    self.preview_lines=vec![e.name.clone(),if !terminal_tree::visual_preview::installed(){"Visual Preview: not installed".into()}else{"Visual Preview: disabled".into()}, "Open Add-ons (Shift+F11)".into(), "Visual Preview installation help".into(), "Alt+P for Info / Hex".into()];
                    return;
                }
                if self.prefs.visual_preview {
                    let page=if terminal_tree::visual_preview::format(&e.path)==Some("pdf"){self.visual_page}else{1};
                    let hd=if super::native_graphics::supported(){terminal_tree::visual_preview::render_hd(&e.path,page,self.visual_size.0,self.visual_size.1).ok()}else{None};
                    self.visual_bitmap=hd.is_some();
                    let result=match hd{Some(f)=>Ok(f),None=>terminal_tree::visual_preview::render(&e.path,page,u32::from(width).clamp(1,160),(rows.saturating_sub(2)*2).clamp(1,160) as u32)};
                    match result {
                        Ok(frame)=>{
                            let label=if terminal_tree::visual_preview::format(&e.path)==Some("pdf"){format!("Page {page}/{} | Scroll | Alt+V",frame.pages)}else{format!("Thumbnail {}x{} {} | Alt+V",frame.width,frame.height,if self.visual_bitmap{"RGB"}else{""})};
                            self.preview_lines=vec![e.name.clone(),label];
                            self.visual_frame=Some(frame);
                        }
                        Err(error)=>self.preview_lines=vec![e.name.clone(),display_name(std::ffi::OsStr::new(&error.to_string()))],
                    }
                    return;
                }
            }
        }
        if self.preview_mode==1 {
            self.preview_lines=match entry {
                None=>vec!["No selection".into()],
                Some(e)=>vec![e.name.clone(),format!("Type: {}",entry_type(e)),format!("Size: {}",human_size(e.size)),format!("Bytes: {}",e.size),format!("Path: {}",display_name(e.path.as_os_str())),if e.symlink{"Symbolic link; not followed".into()}else{"Observed file metadata".into()}],
            };
            return;
        }
        self.preview_lines = match entry {
            None => vec!["No selection".into()],
            Some(e) if e.directory => vec![e.name.clone(), "Folder".into(), "Enter to open".into()],
            Some(e) if e.symlink => vec![e.name.clone(), "Symbolic link".into(), "Preview not followed".into()],
            Some(e) => {
                #[cfg(feature = "native-ops")]
                {
                    match terminal_tree::preview::read_lines(&e.path, if self.preview_mode==2 {terminal_tree::preview::Mode::Hex}else{terminal_tree::preview::Mode::Auto}) {
                        Ok(mut lines) => {
                            if !lines.is_empty() { lines[0] = e.name.clone(); }
                            if self.preview_mode==0 && e.path.extension().and_then(|x|x.to_str()).is_some_and(|x|x.eq_ignore_ascii_case("md")||x.eq_ignore_ascii_case("markdown")) && lines.get(1).is_some_and(|s|s!="Hex preview") {
                                let (formatted,styles)=markdown_preview(&lines,usize::from(width));
                                self.preview_styles=styles;formatted
                            }else{lines}
                        }
                        Err(error) => vec![e.name.clone(), display_name(std::ffi::OsStr::new(&error.to_string()))],
                    }
                }
                #[cfg(not(feature = "native-ops"))]
                vec![e.name.clone(), format!("{} bytes", e.size), "Content preview requires native Linux".into()]
            }
        };
    }

    fn choose_icons(&mut self) -> io::Result<()> {
        use super::file_icons::Style;
        let labels = [
            "Unicode — standard terminal characters".into(),
            "Seti — requires a compatible Nerd Font".into(),
            "ASCII — maximum compatibility".into(),
        ];
        if let Some(index) = super::choose("Icon style", &labels)? {
            self.icons = [Style::Unicode, Style::Seti, Style::Ascii][index];
        }
        Ok(())
    }

    fn choose_view(&mut self, view: View) {
        self.view = view;
        self.dual = false;
        self.expanded_tree = None;
        self.side_focus = false;
    }

    fn start_location(&mut self, model: &Model) {
        self.expanded_tree = None;
        self.side_focus = false;
        self.location = Some(model.directory.to_str().unwrap_or("").to_owned());
    }

    fn open_expanded(&mut self, model: &mut Model) -> io::Result<()> {
        let Some(tree) = &self.expanded_tree else { return Ok(()); };
        let Some(row) = tree.rows().get(tree.selected()) else { return Ok(()); };
        let path = row.entry.path.clone();
        let directory = row.entry.directory && !row.entry.symlink;
        let target = if directory { path.clone() } else {
            path.parent().ok_or_else(|| io::Error::other("missing parent"))?.to_path_buf()
        };
        // Descendant rows are read-only. Navigate into their containing folder,
        // reload ordinary rows and select the exact native path before any write.
        model.go(target)?;
        if !directory {
            if let Some(index) = model.entries.iter().position(|e| e.path == path) {
                model.selected = index;
            }
        }
        self.expanded_tree = None;
        self.preview_key = None;
        self.pane_switched = true;
        Ok(())
    }

    fn expand_current(&mut self, model: &Model) -> io::Result<()> {
        if !model.query.is_empty() || model.kind_filter != KindFilter::All {
            return Err(io::Error::other("clear filters before expanding tree"));
        }
        let Some(entry) = model.entries.get(model.selected) else { return Ok(()); };
        if !entry.directory || entry.symlink { return Ok(()); }
        let mut tree = TreeModel::new(&model.directory, model.hidden, model.order)?;
        let index = tree.rows().iter().position(|r| r.entry.path.file_name() == entry.path.file_name())
            .ok_or_else(|| io::Error::other("tree selection changed; refresh first"))?;
        tree.select(index);
        tree.expand()?;
        self.expanded_tree = Some(tree);
        Ok(())
    }

    fn expanded_input(&mut self, event: &Event, model: &mut Model, l: Layout) -> io::Result<bool> {
        let Some(tree) = self.expanded_tree.as_mut() else { return Ok(false); };
        let cells = Cells::new(l.end()-l.first(), l.capacity(), View::Tree, self.zoom);
        match event {
            Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if k.modifiers == KeyModifiers::NONE {
                    match k.code {
                        KeyCode::Up => tree.shift(-1),
                        KeyCode::Down => tree.shift(1),
                        KeyCode::Home => tree.select(0),
                        KeyCode::End => tree.select(usize::MAX),
                        KeyCode::PageUp => tree.shift(-(cells.capacity() as isize)),
                        KeyCode::PageDown => tree.shift(cells.capacity() as isize),
                        KeyCode::Right => tree.expand()?,
                        KeyCode::Left => tree.collapse_or_parent(),
                        KeyCode::Enter => self.open_expanded(model)?,
                        KeyCode::Esc => { self.expanded_tree = None; },
                        KeyCode::F(5) => { tree.refresh()?; },
                        KeyCode::F(8) | KeyCode::F(9) | KeyCode::F(10) | KeyCode::Char('q') => {
                            self.expanded_tree = None;
                            return Ok(false);
                        }
                        _ => return Err(io::Error::other("Expanded tree: Enter opens folder or selects file in its parent; Esc returns")),
                    }
                } else if k.modifiers == KeyModifiers::CONTROL && matches!(k.code, KeyCode::Char('l' | 'f' | 'q')) {
                    self.expanded_tree = None;
                    return Ok(false);
                }
            }
            Event::Mouse(mouse) if l.file_row(mouse.column, mouse.row).is_some() => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => tree.shift(-1),
                    MouseEventKind::ScrollDown => tree.shift(1),
                    MouseEventKind::Down(MouseButton::Left) => {
                        let (start,end) = tree.viewport(cells.capacity());
                        if let Some(offset) = cells.hit(mouse.column-l.first(), usize::from(mouse.row-4)) {
                            let index = start + offset;
                            if index < end {
                                tree.select(index);
                                if mouse.column < l.first()+4 {
                                    if tree.rows()[index].expanded { tree.collapse_or_parent(); }
                                    else { tree.expand()?; }
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.expanded_tree = None;
                return Ok(false);
            }
            _ => (),
        }
        Ok(true)
    }

    fn toggle_dual(&mut self, model: &Model) -> io::Result<()> {
        if !self.dual && self.other.is_none() {
            let mut other = Model::new(model.directory.clone())?;
            other.set_view(model.hidden, model.order, model.kind_filter, String::new())?;
            self.other = Some(other);
        }
        self.expanded_tree = None;
        self.dual = !self.dual;
        self.side_focus = false;
        self.preview_key = None;
        Ok(())
    }

    fn switch_pane(&mut self, model: &mut Model) -> io::Result<()> {
        let other = self.other.as_mut().ok_or_else(|| io::Error::other("second pane unavailable"))?;
        // Refresh the incoming pane before making its rows actionable.
        // A failed refresh leaves the active model and selection unchanged.
        other.reload()?;
        std::mem::swap(model, other);
        self.active_right = !self.active_right;
        self.pane_switched = true;
        self.preview_key = None;
        Ok(())
    }

    pub fn take_pane_switch(&mut self) -> bool {
        std::mem::take(&mut self.pane_switched)
    }

    fn draw_dual(
        &mut self,
        model: &mut Model,
        message: &str,
        searching: bool,
        writable: bool,
        personal: bool,
        clipboard: usize,
        marked: &impl Fn(&std::path::Path) -> bool,
    ) -> io::Result<()> {
        let (w, full_h) = terminal::size()?;
        let h=full_h-self.panel_rows(full_h);
        let width = w.saturating_sub(1);
        let mut out = Vec::new();
        queue!(out, MoveTo(0, 0), SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background), Clear(ClearType::All),
            SetForegroundColor(self.prefs.accent),
            Print(super::clip("Terminal Tree  File  Edit  View  Go  Tools  Settings  Add-ons", usize::from(width))),
            SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background))?;
        #[cfg(feature = "native-ops")]
        self.terminal_button(&mut out,width)?;
        if h < 8 { queue!(out,ResetColor)?; return self.frame_cache.present(out,w,full_h,self.prefs.background); }
        let side = if self.active_right { "Right" } else { "Left" };
        let mode = if personal { "Personal" } else if writable { "Workspace" } else { "Read-only" };
        let header = format!("Dual Pane | Active: {side} | [{} / {mode}]", super::edition::NAME);
        queue!(out, MoveTo(0, 1), Print(super::clip(&header, usize::from(width))))?;
        self.draw_tabs(&mut out,model,width)?;
        let cap = usize::from(h - 7);
        if width >= 54 {
            let split = dual_split(width,self.prefs.dual_percent);
            let other = self.other.as_mut().ok_or_else(|| io::Error::other("second pane unavailable"))?;
            if self.active_right {
                draw_column(&mut out, other, "Left", false, 0, split, cap, marked, self.icons, &self.prefs)?;
                draw_column(&mut out, model, "Right", true, split + 1, width - split - 1, cap, marked, self.icons, &self.prefs)?;
            } else {
                draw_column(&mut out, model, "Left", true, 0, split, cap, marked, self.icons, &self.prefs)?;
                draw_column(&mut out, other, "Right", false, split + 1, width - split - 1, cap, marked, self.icons, &self.prefs)?;
            }
            for row in 3..h - 3 {
                queue!(out, MoveTo(split, row), Print("│"))?;
            }
        } else {
            draw_column(&mut out, model, side, true, 0, width, cap, marked, self.icons, &self.prefs)?;
        }
        let safety = if writable { "Scoped" } else { "Read-only" };
        let disk=if self.prefs.status_details {self.disk_status(&model.directory)}else{String::new()};
        let status = if self.prefs.status_details { format!("Clipboard: {clipboard} files | Tasks: {} | Safety: {safety} | {} entries | {disk}",self.task_label(), model.entries.len()) } else { format!("Safety: {safety}") };
        let detail = if searching || !model.query.is_empty() {
            format!("Filter: {} | {message}", display_name(std::ffi::OsStr::new(&model.query)))
        } else { message.to_owned() };
        queue!(out, MoveTo(0, h - 3), Print(super::clip(&status, usize::from(width))),
            MoveTo(0, h - 2), Print(super::clip(&detail, usize::from(width))),
            MoveTo(0, h - 1), Print(super::clip(if searching {
                "Filter: type text | Enter accept | Esc cancel"
            } else if writable {
                "F8 Single  Tab Switch  Ctrl+C/X/V Transfer  F4 Actions  F10 Menu"
            } else {
                "F8 Single  Tab Switch  F10 Menu  Ctrl+F Filter  q Exit"
            }, usize::from(width))))?;
        #[cfg(feature = "native-ops")]
        self.draw_terminal(&mut out,full_h,w.saturating_sub(1),model)?;
        queue!(out,ResetColor)?;
        self.frame_cache.present(out,w,full_h,self.prefs.background)
    }

    pub fn draw(
        &mut self, model: &mut Model, message: &str, searching: bool,
        writable: bool, personal: bool, clipboard: usize, marked: impl Fn(&std::path::Path) -> bool,
    ) -> io::Result<()> {
        if self.dual {
            #[cfg(feature = "native-ops")]
            {self.graphics_slot.clear()?;self.graphics_placed=false;}
            return self.draw_dual(model, message, searching, writable, personal, clipboard, &marked);
        }
        let (w,full_h) = terminal::size()?;
        let h=full_h-self.panel_rows(full_h);
        let l = self.layout(w,h);
        if l.sidebar == 0 { self.side_focus = false; }
        let mut out = Vec::new();
        queue!(out, MoveTo(0,0), SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background), Clear(ClearType::All),
            SetForegroundColor(self.prefs.accent), Print(super::clip("Terminal Tree  File  Edit  View  Go  Tools  Settings  Add-ons", usize::from(l.width))), SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background))?;
        #[cfg(feature = "native-ops")]
        self.terminal_button(&mut out,l.width)?;
        if h < 8 { #[cfg(feature = "native-ops")] {self.graphics_slot.clear()?;self.graphics_placed=false;} queue!(out,ResetColor)?; return self.frame_cache.present(out,w,full_h,self.prefs.background); }
        let mode = if personal { "Personal" } else if writable { "Workspace" } else { "Read-only" };
        let path_width = if self.prefs.toolbar { draw_toolbar(&mut out,l.width,self.view,&self.prefs)? } else { l.width };
        let path = if let Some(input) = &self.location {
            format!("Location: {}_", display_name(std::ffi::OsStr::new(input)))
        } else {
            breadcrumb(&model.directory)
        };
        queue!(out, MoveTo(0,1), Print(super::clip(&path,usize::from(path_width))))?;
        self.draw_tabs(&mut out,model,l.width)?;
        if l.sidebar > 0 {
            queue!(out, MoveTo(0,3), SetForegroundColor(self.prefs.accent),
                Print(if self.side_focus { "Places *" } else { "Places" }), SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background))?;
        }
        queue!(out, MoveTo(l.first(),3), SetForegroundColor(self.prefs.accent),
            Print(if self.view==View::List {list_heading(model.order,!self.side_focus,l.end()-l.first(),self.prefs.list_details)}else if self.side_focus {"Files".into()}else{"Files *".into()}), SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background))?;
        if l.preview > 0 {
            queue!(out, MoveTo(l.end()+1,3), SetForegroundColor(self.prefs.accent), Print(super::clip(match self.preview_mode {1=>"Preview [Info]",2=>"Preview [Hex]",_=>"Preview [Auto]"},usize::from(l.preview))), SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background))?;
            let entry = if let Some(tree) = &self.expanded_tree {
                tree.rows().get(tree.selected()).map(|r| r.entry.clone())
            } else { model.entries.get(model.selected).cloned() };
            self.preview(entry.as_ref(),l.preview,l.capacity());
        }
        let cap = l.capacity();
        let cells = Cells::new(l.end()-l.first(), cap, self.view, self.zoom);
        let (start,end) = if let Some(tree) = &mut self.expanded_tree {
            tree.viewport(cells.capacity())
        } else { model.viewport(cells.capacity()) };
        if self.side_cursor < self.side_top { self.side_top = self.side_cursor; }
        if self.side_cursor >= self.side_top + cap { self.side_top = self.side_cursor + 1 - cap; }
        self.side_top = self.side_top.min(self.places.len().saturating_sub(cap));
        for row in 0..cap {
            let y = row as u16 + 4;
            if l.sidebar > 0 {
                if let Some((label,path)) = self.places.get(self.side_top+row) {
                    queue!(out, MoveTo(0,y))?;
                    if path.is_none() { queue!(out, SetForegroundColor(self.prefs.accent))?; }
                    else if self.side_focus && self.side_top+row == self.side_cursor {
                        queue!(out, SetBackgroundColor(self.prefs.selection))?;
                    }
                    let text = if let Some(path) = path {
                        let (glyph, _) = self.prefs.icon(path, true, false, self.icons);
                        format!(" {glyph} {label}")
                    } else { label.clone() };
                    queue!(out, Print(super::clip(&text,usize::from(l.sidebar))), SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background))?;
                }
                queue!(out, MoveTo(l.sidebar,y), Print("│"))?;
            }
            if l.preview > 0 {
                queue!(out, MoveTo(l.end(),y), Print("│"))?;
                if let Some(line) = self.preview_lines.get(row+self.preview_scroll) {
                    let style=self.preview_styles.get(row+self.preview_scroll).copied().unwrap_or_default();
                    queue!(out,MoveTo(l.end()+1,y))?;
                    match style {
                        PreviewStyle::Heading=>queue!(out,SetAttribute(Attribute::Bold),SetForegroundColor(self.prefs.accent))?,
                        PreviewStyle::Code=>queue!(out,SetForegroundColor(Color::Yellow))?,
                        PreviewStyle::Quote=>queue!(out,SetForegroundColor(self.prefs.accent))?,
                        PreviewStyle::Plain=>(),
                    }
                    queue!(out,Print(super::clip(line,usize::from(l.preview))),SetAttribute(Attribute::NormalIntensity),SetForegroundColor(self.prefs.foreground))?;
                } else {
                    #[cfg(feature = "native-ops")]
                    if let Some(frame)=self.visual_frame.as_ref().filter(|_|!self.visual_bitmap) {
                        let py=row.saturating_sub(2)*2;
                        if py<frame.height {
                            queue!(out,MoveTo(l.end()+1,y))?;
                            for x in 0..frame.width.min(usize::from(l.preview)) {
                                let [r,g,b]=frame.pixel(x,py);
                                let [r2,g2,b2]=frame.pixel(x,py+1);
                                queue!(out,SetForegroundColor(Color::Rgb{r,g,b}),SetBackgroundColor(Color::Rgb{r:r2,g:g2,b:b2}),Print("▀"))?;
                            }
                            queue!(out,SetForegroundColor(self.prefs.foreground),SetBackgroundColor(self.prefs.background))?;
                        }
                    }
                }
            }
        }
        for index in start..end {
            let offset = index-start;
            let x = l.first() + (offset % cells.columns) as u16 * cells.width;
            let y = 4 + ((offset / cells.columns) * cells.stride) as u16;
            if y >= h-3 { break; }
            let (e, selected, tree_depth, expanded) = if let Some(tree) = &self.expanded_tree {
                let row = &tree.rows()[index];
                (&row.entry, tree.selected() == index, Some(row.depth), row.expanded)
            } else { (&model.entries[index], model.selected == index, None, false) };
            let (glyph,color) = self.prefs.icon(&e.path,e.directory,e.symlink,self.icons);
            queue!(out, MoveTo(x,y))?;
            if selected { queue!(out, SetBackgroundColor(self.prefs.selection))?; }
            queue!(out, SetForegroundColor(color))?;
            if self.view == View::Grid {
                let mark = if marked(&e.path) { "[x] " } else { "" };
                queue!(out, Print(super::clip(&format!("{mark}{glyph}"),usize::from(cells.width))))?;
                if y+1 < h-3 {
                    queue!(out, MoveTo(x,y+1), Print(super::clip(&e.name,usize::from(cells.width))))?;
                }
            } else {
                let text = if let Some(depth) = tree_depth {
                    format!("{}{} {glyph} {}", "  ".repeat(depth-1),
                        if expanded { "−" } else if e.directory && !e.symlink { "+" } else { "·" }, e.name)
                } else { row_text(e,index,model.entries.len(),self.view,glyph,marked(&e.path),cells.width,self.prefs.list_details) };
                queue!(out, Print(super::clip(&text,usize::from(cells.width))))?;
            }
            queue!(out, SetForegroundColor(self.prefs.foreground), SetBackgroundColor(self.prefs.background))?;
        }
        let safety = if self.expanded_tree.is_some() { "Tree inspection" } else if writable { "Scoped" } else { "Read-only" };
        let disk=if self.prefs.status_details {self.disk_status(&model.directory)}else{String::new()};
        let status = if self.prefs.status_details { format!("Clipboard: {clipboard} files | Tasks: {} | Safety: {safety} | {} | {} View | Zoom {} | {disk}",self.task_label(),model.entries.len(),self.view.name(),self.zoom) } else { format!("Safety: {safety} | {} View | Zoom {}",self.view.name(),self.zoom) };
        let detail = if self.expanded_tree.is_some() {
            "Expanded tree: Right expand / Left collapse / Enter open / Esc back".to_owned()
        } else if searching || !model.query.is_empty() {
            format!("Search: {} | {message}", display_name(std::ffi::OsStr::new(&model.query)))
        } else { format!("[{} / {mode}] Icons: {} | {message}",super::edition::NAME,self.icons.name()) };
        queue!(out, MoveTo(0,h-3), Print(super::clip(&status,usize::from(l.width))),
            MoveTo(0,h-2), Print(super::clip(&detail,usize::from(l.width))),
            MoveTo(0,h-1), Print(super::clip(if searching {
                "Filter: type text | Enter accept | Esc cancel"
            } else if writable {
                "F1 Help  F2 Rename  F3 Preview  F8 Dual  Ctrl+T Tab  Ctrl+K Commands"
            } else {
                "F1 Help  F3 Preview  F8 Dual  Ctrl+T Tab  Ctrl+K Commands  q Exit"
            },usize::from(l.width))))?;
        #[cfg(feature = "native-ops")]
        self.draw_terminal(&mut out,full_h,w.saturating_sub(1),model)?;
        queue!(out,ResetColor)?;
        #[cfg(feature = "native-ops")]
        if l.preview==0||!self.visual_bitmap||self.visual_frame.is_none()||!self.graphics_placed{self.graphics_slot.clear()?;self.graphics_placed=false;}
        self.frame_cache.present(out,w,full_h,self.prefs.background)?;
        #[cfg(feature = "native-ops")]
        if l.preview>0 && self.visual_bitmap && !self.graphics_placed && cap>2 {
            if let Some(frame)=&self.visual_frame{
                let (cols,rows)=super::native_graphics::fit(frame,l.preview,(cap-2).min(u16::MAX as usize) as u16);
                self.graphics_slot.show(frame,l.end()+1+(l.preview-cols)/2,6,cols,rows)?;
                self.graphics_placed=true;
            }
        }
        Ok(())
    }

    fn menu(&mut self, category: Option<usize>, writable: bool) -> io::Result<Option<Event>> {
        let category = match category {
            Some(i) => i,
            None => {
                let labels = MENUS.map(|(name,_)| name.to_owned());
                match super::choose("Menu", &labels)? { Some(i) => i, None => return Ok(None) }
            }
        };
        let commands = self.commands(category,writable);
        // Settings and Add-ons remain direct one-click destinations.
        if category == 5 || category == 6 {
            let (_,key,mods)=commands[0];
            return Ok(Some(Event::Key(KeyEvent::new(key,mods))));
        }
        let labels: Vec<_> = commands.iter().map(|(label,_,_)| label.to_string()).collect();
        Ok(super::choose(MENUS[category].0,&labels)?.map(|i| {
            let (_,key,modifiers) = commands[i];
            Event::Key(KeyEvent::new(key,modifiers))
        }))
    }

    fn commands(&self,category:usize,writable:bool)->Vec<(&'static str,KeyCode,KeyModifiers)> {
        let plain=KeyModifiers::NONE;
        let mut commands = match category {
            0 => vec![("New tab (Ctrl+T)",KeyCode::Char('t'),KeyModifiers::CONTROL),("Close tab (Ctrl+W)",KeyCode::Char('w'),KeyModifiers::CONTROL),("Next tab (Ctrl+Tab)",KeyCode::Tab,KeyModifiers::CONTROL),("Open",KeyCode::Enter,plain),("Properties",KeyCode::Char('i'),plain),("Exit",KeyCode::Char('q'),plain)],
            1 if writable => vec![("Copy (Ctrl+C)",KeyCode::Char('c'),plain),("Cut (Ctrl+X)",KeyCode::Char('x'),plain),("Paste (Ctrl+V)",KeyCode::Char('v'),plain),("View clipboard",KeyCode::F(12),plain),("Rename",KeyCode::F(2),plain),("New folder",KeyCode::Char('n'),plain),("New file",KeyCode::Char('t'),plain),("Trash",KeyCode::Delete,plain),("Undo",KeyCode::Char('u'),plain)],
            1 => vec![("Select a scoped workspace to enable file operations",KeyCode::Null,plain)],
            2 => vec![("Cycle preview mode (Alt+P)",KeyCode::Char('p'),KeyModifiers::ALT),("Tree view (Ctrl+3)",KeyCode::Char('3'),KeyModifiers::CONTROL),("Grid view (Ctrl+1)",KeyCode::Char('1'),KeyModifiers::CONTROL),("List view (Ctrl+2)",KeyCode::Char('2'),KeyModifiers::CONTROL),("Toggle dual pane (F8)",KeyCode::F(8),plain),("Toggle live preview",KeyCode::F(9),plain),("Open preview",KeyCode::F(3),plain),("Hidden files",KeyCode::Char('h'),plain),("Sort",KeyCode::Char('s'),plain),("Refresh",KeyCode::F(5),plain),("Filter",KeyCode::Char('/'),plain)],
            3 => vec![("Parent",KeyCode::Left,plain),("Back",KeyCode::Char('['),plain),("Forward",KeyCode::Char(']'),plain),("Locations",KeyCode::Char('m'),plain),("Bookmark current directory",KeyCode::Char('b'),plain),("Bookmarks",KeyCode::Char('B'),plain)],
            4 => vec![("Companion tools",KeyCode::Char('p'),KeyModifiers::CONTROL),("Git Manager (Alt+G)",KeyCode::Char('g'),KeyModifiers::ALT),("Help",KeyCode::F(1),plain)],
            5 => vec![("Settings Center",KeyCode::Char(','),KeyModifiers::CONTROL)],
            _ => vec![("Manage integrations",KeyCode::F(11),KeyModifiers::SHIFT)],
        };
        if category == 2 && cfg!(feature = "native-ops") {
            commands.push(("Toggle terminal panel (Alt+H)",KeyCode::Char('h'),KeyModifiers::ALT));
            commands.push(("Focus terminal (Alt+T)",KeyCode::Char('t'),KeyModifiers::ALT));
            commands.push(("Expand visual preview (Alt+V)",KeyCode::Char('v'),KeyModifiers::ALT));
            commands.push(("Start or close interactive shell (Alt+S)",KeyCode::Char('s'),KeyModifiers::ALT));
            commands.push(("Hex preview (Shift+F8)", KeyCode::F(8), KeyModifiers::SHIFT));
        }
        if category == 3 {
            commands.insert(0,("Go to path (Ctrl+L)",KeyCode::Char('l'),KeyModifiers::CONTROL));
        }
        if category == 4 && writable { commands.push(("File actions",KeyCode::F(6),plain)); commands.push(("Recovery",KeyCode::Char('j'),plain)); }

        if !cfg!(feature = "native-ops") {
            commands.retain(|(_,key,mods)| *key != KeyCode::F(3) && !(*mods == KeyModifiers::CONTROL && *key == KeyCode::Char('p')) && !(*mods == KeyModifiers::ALT && *key == KeyCode::Char('g')));
        }
        commands.retain(|(_,key,_)|*key!=KeyCode::Null);
        // Keep a readable explanation in the Edit menu when writes are disabled.
        if commands.is_empty() { commands.push(("Select a scoped workspace to enable file operations",KeyCode::Null,plain)); }
        commands
    }

    fn palette(&self,writable:bool)->io::Result<Option<Event>> {
        let mut actions=Vec::new();
        let mut labels=Vec::new();
        for (category,(name,_)) in MENUS.iter().enumerate() {
            for (label,key,mods) in self.commands(category,writable) {
                if key==KeyCode::Null { continue; }
                labels.push(format!("{name} / {label}"));
                actions.push(Event::Key(KeyEvent::new(key,mods)));
            }
        }
        Ok(super::command_palette::choose(&labels)?.map(|i|actions.remove(i)))
    }

    fn context_menu(&self, writable: bool) -> io::Result<Option<Event>> {
        let mut actions = vec![
            ("Open", KeyCode::Enter),
            ("Properties", KeyCode::Char('i')),
        ];
        if cfg!(feature = "native-ops") {
            actions.push(("Preview", KeyCode::F(3)));
        }
        if writable {
            actions.extend([
                ("Copy", KeyCode::Char('c')),
                ("Cut", KeyCode::Char('x')),
                ("Paste", KeyCode::Char('v')),
                ("Rename", KeyCode::F(2)),
                ("Select / deselect", KeyCode::Char(' ')),
                ("Move to Trash", KeyCode::Delete),
                ("More actions", KeyCode::F(6)),
            ]);
        }
        let labels: Vec<_> = actions.iter().map(|(label, _)| label.to_string()).collect();
        Ok(super::choose("File context menu", &labels)?.map(|i| {
            Event::Key(KeyEvent::new(actions[i].1, KeyModifiers::NONE))
        }))
    }

    /// Handle shell-only interactions; translate file-list coordinates back to
    /// the existing navigator so the original review and write engine is used.
    pub fn input(&mut self, mut event: Event, model: &mut Model, searching: bool, writable: bool) -> io::Result<Option<Event>> {
        // Modal actions can repaint the physical screen. Plain navigation and
        // passive mouse events cannot, so keep the row cache for those.
        let passive=matches!(&event,Event::Mouse(mouse) if matches!(mouse.kind,MouseEventKind::Moved|MouseEventKind::Up(_)));
        let arrow=matches!(&event,Event::Key(k) if k.modifiers==KeyModifiers::NONE && matches!(k.code,KeyCode::Up|KeyCode::Down));
        if !passive && !(arrow&&!self.dual&&!searching&&self.location.is_none()&&self.expanded_tree.is_none()) {self.frame_cache.invalidate();
            #[cfg(feature = "native-ops")]
            {self.graphics_slot.clear()?;self.graphics_placed=false;}
        }
        let (w,full_h) = terminal::size()?;
        let h=full_h-self.panel_rows(full_h);
        let l = self.layout(w,h);
        if searching { return Ok(Some(event)); }
        #[cfg(feature = "native-ops")]
        {
            if self.panel_rows(full_h)==0 {self.terminal_panel.focused=false;}
            if let Event::Mouse(mouse)=&event {
                if self.location.is_none()&&mouse.row==0&&l.width>=83&&(78..83).contains(&mouse.column)&&mouse.kind==MouseEventKind::Down(MouseButton::Left)&&mouse.modifiers==KeyModifiers::NONE {
                    self.terminal_panel.focused=false;self.expanded_tree=None;self.side_focus=false;
                    super::git_ui::show(&model.directory,self.prefs.companions[2])?;
                    model.reload()?;self.pane_switched=true;self.preview_key=None;return Ok(None);
                }
                if matches!(self.dragging,Some(Divider::Terminal)) {
                    match mouse.kind {
                        MouseEventKind::Drag(MouseButton::Left)=>self.resize_drag(Divider::Terminal,mouse.column,l.width,mouse.row,full_h),
                        MouseEventKind::Up(MouseButton::Left)=>{self.resize_drag(Divider::Terminal,mouse.column,l.width,mouse.row,full_h);self.dragging=None;},
                        _=>self.dragging=None,
                    }
                    return Ok(None);
                }
                if self.location.is_none() && mouse.kind==MouseEventKind::Down(MouseButton::Left) && mouse.modifiers==KeyModifiers::NONE {
                    if mouse.row==0 && l.width>=75 && mouse.column>=63 && mouse.column<75 {
                        self.prefs.terminal_visible=!self.prefs.terminal_visible;self.terminal_panel.focused=false;return Ok(None);
                    }
                    if self.panel_rows(full_h)>0 && mouse.row==h && l.width>=84 && mouse.column>=l.width-8 && mouse.column<l.width {
                        self.dragging=Some(Divider::Terminal);return Ok(None);
                    }
                }
            }
            if self.location.is_none() {
                if let Event::Key(k)=&event {
                    if k.kind==KeyEventKind::Press && k.modifiers==KeyModifiers::ALT {
                        match k.code {
                            KeyCode::Char('s')=>{
                                if self.terminal_panel.interactive(){
                                    if super::choose("Close interactive shell? Running jobs may be interrupted",&["Cancel".into(),"Close shell".into()])?==Some(1){
                                        self.terminal_panel.close_shell();model.reload()?;self.pane_switched=true;self.preview_key=None;
                                    }
                                }else{
                                    if full_h<16{return Err(io::Error::other("terminal panel needs at least 16 rows"));}
                                    if super::choose("Interactive shell uses OS permissions, outside file journal",&["Cancel".into(),"Start Bash PTY".into()])?==Some(1){
                                        self.prefs.terminal_visible=true;
                                        self.terminal_panel.start_shell(&model.directory,l.width,self.panel_rows(full_h).saturating_sub(3))?;
                                        self.side_focus=false;
                                    }
                                }
                                return Ok(None);
                            }
                            KeyCode::Char('h')=>{self.prefs.terminal_visible=!self.prefs.terminal_visible;self.terminal_panel.focused=false;return Ok(None);}
                            KeyCode::Char('t')=>{self.prefs.terminal_visible=true;if self.panel_rows(full_h)==0 {return Err(io::Error::other("terminal panel needs at least 16 rows"));}self.terminal_panel.focused=!self.terminal_panel.focused;self.side_focus=false;return Ok(None);}
                            _=>(),
                        }
                    }
                    if self.terminal_panel.focused {
                        self.terminal_panel.input(*k,&model.directory)?;
                        return Ok(None);
                    }
                }
                if let Event::Mouse(mouse)=&event {
                    if self.panel_rows(full_h)>0 && mouse.row>=h {
                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left) if mouse.row==h && mouse.column<6=>{self.prefs.terminal_visible=false;self.terminal_panel.focused=false;}
                            MouseEventKind::Down(MouseButton::Left)=>{self.terminal_panel.focused=true;self.side_focus=false;}
                            MouseEventKind::ScrollUp=>self.terminal_panel.scroll(true),
                            MouseEventKind::ScrollDown=>self.terminal_panel.scroll(false),
                            _=>(),
                        }
                        return Ok(None);
                    }
                    if self.terminal_panel.focused && matches!(mouse.kind,MouseEventKind::Down(_)) {self.terminal_panel.focused=false;}
                }
            }
        }
        if let Some(input) = &mut self.location {
            match &event {
                Event::Key(k) if matches!(k.kind,KeyEventKind::Press | KeyEventKind::Repeat) => {
                    match k.code {
                        KeyCode::Esc => self.location = None,
                        KeyCode::Char('l' | 'c') if k.modifiers == KeyModifiers::CONTROL => self.location = None,
                        KeyCode::Char('a') if k.modifiers == KeyModifiers::CONTROL => input.clear(),
                        KeyCode::Backspace => { input.pop(); }
                        KeyCode::Enter => {
                            if input.is_empty() { return Ok(None); }
                            let path = if input == "~" || input.starts_with("~/") {
                                super::platform::home_dir().ok_or_else(|| io::Error::other("home unavailable"))?
                                    .join(input.strip_prefix("~/").unwrap_or(""))
                            } else { model.directory.join(input.as_str()) };
                            model.go(path.canonicalize()?)?;
                            self.location = None;
                            self.preview_key = None;
                        }
                        KeyCode::Char(c) if !c.is_control() && !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && input.len()+c.len_utf8() <= 4096 => input.push(c),
                        _ => (),
                    }
                }
                Event::Mouse(mouse) if mouse.row == 1 && mouse.kind == MouseEventKind::Down(MouseButton::Left) && toolbar_hit(l.width,mouse.column) == Some(0) => self.location = None,
                _ => (),
            }
            return Ok(None);
        }
        if let Event::Mouse(mouse) = &event {
            if !self.dual && mouse.modifiers.contains(KeyModifiers::CONTROL) {
                match mouse.kind {
                    MouseEventKind::ScrollUp => { self.zoom = (self.zoom+1).min(3); return Ok(None); }
                    MouseEventKind::ScrollDown => { self.zoom = self.zoom.saturating_sub(1).max(1); return Ok(None); }
                    _ => (),
                }
            }
            if !self.dual && mouse.row == 1 && mouse.kind == MouseEventKind::Down(MouseButton::Left) && mouse.modifiers==KeyModifiers::NONE {
                let path_width=if self.prefs.toolbar{toolbar_start(l.width).unwrap_or(l.width)}else{l.width};
                if mouse.column<path_width {
                    if let Some(target)=breadcrumb_hit(&model.directory,mouse.column,path_width){
                        if target!=model.directory {model.go(target)?;self.clear_tab_transients();}
                    }
                    return Ok(None);
                }
                if !self.prefs.toolbar{return Ok(None);}
                match toolbar_hit(l.width, mouse.column) {
                    Some(0) => self.start_location(model),
                    Some(1) => {
                        self.expanded_tree = None;
                        self.side_focus = false;
                        return Ok(Some(Event::Key(KeyEvent::new(KeyCode::Char('/'),KeyModifiers::NONE))));
                    }
                    Some(2) => self.choose_view(View::Grid),
                    Some(3) => self.choose_view(View::List),
                    Some(4) => self.choose_view(View::Tree),
                    _ => (),
                }
                return Ok(None);
            }
        }
        if let Event::Key(k) = &event {
            if k.modifiers == KeyModifiers::CONTROL && matches!(k.kind,KeyEventKind::Press | KeyEventKind::Repeat) {
                match k.code {
                    KeyCode::Char('1') => { self.choose_view(View::Grid); return Ok(None); }
                    KeyCode::Char('2') => { self.choose_view(View::List); return Ok(None); }
                    KeyCode::Char('3') => { self.choose_view(View::Tree); return Ok(None); }
                    KeyCode::Char('l') if !self.dual => { self.start_location(model); return Ok(None); }
                    KeyCode::Char('+' | '=') if !self.dual => { self.zoom = (self.zoom+1).min(3); return Ok(None); }
                    KeyCode::Char('-') if !self.dual => { self.zoom = self.zoom.saturating_sub(1).max(1); return Ok(None); }
                    KeyCode::Char('0') if !self.dual => { self.zoom = 1; return Ok(None); }
                    _ => (),
                }
            }
        }
        if let Event::Key(k) = &event {
            if k.code == KeyCode::F(11) && k.modifiers == KeyModifiers::NONE && k.kind == KeyEventKind::Press {
                self.choose_icons()?;
                return Ok(None);
            }
        }
        if let Event::Key(k) = &event {
            if k.kind == KeyEventKind::Press {
                if k.code == KeyCode::Char('k') && k.modifiers == KeyModifiers::CONTROL {
                    self.expanded_tree=None;
                    self.side_focus=false;
                    let Some(selected)=self.palette(writable)? else { return Ok(None); };
                    return self.input(selected,model,false,writable);
                }
                if k.code == KeyCode::Char(',') && k.modifiers == KeyModifiers::CONTROL {
                    self.expanded_tree=None;
                    self.settings_center(model)?;
                    return Ok(None);
                }
                if k.code == KeyCode::F(11) && k.modifiers == KeyModifiers::SHIFT {
                    self.integrations()?;
                    return Ok(None);
                }
                #[cfg(feature = "native-ops")]
                if k.code == KeyCode::Char('v') && k.modifiers == KeyModifiers::ALT {
                    if !self.prefs.visual_preview{return Err(io::Error::other("Enable Visual Preview in Add-ons first"));}
                    let entry=if let Some(tree)=&self.expanded_tree{tree.rows().get(tree.selected()).map(|r|&r.entry)}else{model.entries.get(model.selected)};
                    if let Some(e)=entry.filter(|e|!e.directory&&!e.symlink&&terminal_tree::visual_preview::format(&e.path).is_some()){super::visual_ui::show(&e.path)?;}
                    return Ok(None);
                }
                #[cfg(feature = "native-ops")]
                if k.code == KeyCode::Char('g') && k.modifiers == KeyModifiers::ALT {
                    self.expanded_tree=None;self.side_focus=false;
                    super::git_ui::show(&model.directory,self.prefs.companions[2])?;
                    model.reload()?;self.pane_switched=true;self.preview_key=None;
                    return Ok(None);
                }
                #[cfg(feature = "native-ops")]
                if k.code == KeyCode::Char('p') && k.modifiers == KeyModifiers::CONTROL {
                    self.expanded_tree=None;
                    super::tools_ui::show_filtered(&model.directory,model.entries.get(model.selected).map(|e| e.path.as_path()),self.prefs.companions,&mut self.last_tool)?;
                    return Ok(None);
                }
            }
        }
        if let Event::Key(k)=&event {
            if k.kind==KeyEventKind::Press && k.modifiers.contains(KeyModifiers::CONTROL) {
                match k.code {
                    KeyCode::Char('t') if k.modifiers==KeyModifiers::CONTROL=>{self.new_tab(model)?;return Ok(None);}
                    KeyCode::Char('w') if k.modifiers==KeyModifiers::CONTROL=>{self.close_tab(model)?;return Ok(None);}
                    KeyCode::Tab|KeyCode::BackTab=>{
                        let n=self.tabs.len();
                        let index=if k.modifiers.contains(KeyModifiers::SHIFT)||k.code==KeyCode::BackTab {(self.active_tab+n-1)%n}else{(self.active_tab+1)%n};
                        self.switch_tab(model,index)?;return Ok(None);
                    }
                    _=>(),
                }
            }
        }
        if let Event::Mouse(mouse)=&event {
            if let Some(divider)=self.dragging {
                match mouse.kind {
                    MouseEventKind::Drag(MouseButton::Left)=>self.resize_drag(divider,mouse.column,l.width,mouse.row,full_h),
                    MouseEventKind::Up(MouseButton::Left)=>{self.resize_drag(divider,mouse.column,l.width,mouse.row,full_h);self.dragging=None;},
                    _=>self.dragging=None,
                }
                return Ok(None);
            }
            if mouse.kind==MouseEventKind::Down(MouseButton::Left) && mouse.modifiers==KeyModifiers::NONE {
                if mouse.row==2 && l.width>=8 {
                    let (first,cell,count)=self.tab_slots(l.width);
                    let slot=usize::from(mouse.column/cell);
                    if slot<count {self.switch_tab(model,first+slot)?;}
                    else if mouse.column>=count as u16*cell&&mouse.column<count as u16*cell+3 {self.new_tab(model)?;}
                    return Ok(None);
                }
                if h>=8 && mouse.row>=3&&mouse.row<h-3 {
                    self.dragging=if self.dual {
                        (l.width>=54&&mouse.column==dual_split(l.width,self.prefs.dual_percent)).then_some(Divider::Dual)
                    } else if l.sidebar>0&&mouse.column==l.sidebar {Some(Divider::Sidebar)}
                    else if l.preview>0&&mouse.column==l.end(){Some(Divider::Preview)}else{None};
                    if self.dragging.is_some(){return Ok(None);}
                }
            }
        }
        if !self.dual && self.view==View::List {
            if let Event::Mouse(mouse)=&event {
                if h>=8 && mouse.row==3 && (l.first()..l.end()).contains(&mouse.column) && mouse.kind==MouseEventKind::Down(MouseButton::Left)&&mouse.modifiers==KeyModifiers::NONE {
                    let order=list_sort_hit(mouse.column-l.first(),l.end()-l.first(),self.prefs.list_details);
                    model.set_view(model.hidden,order,model.kind_filter,model.query.clone())?;
                    self.pane_switched=true;self.side_focus=false;self.expanded_tree=None;self.preview_key=None;
                    return Ok(None);
                }
            }
        }
        if !self.dual && l.preview>0 {
            if let Event::Key(k)=&event {
                if k.kind==KeyEventKind::Press&&k.modifiers==KeyModifiers::ALT&&k.code==KeyCode::Char('p') {
                    self.preview_mode=(self.preview_mode+1)%3;
                    self.preview_key=None;self.preview_scroll=0;
                    return Ok(None);
                }
            }
            if let Event::Mouse(mouse)=&event {
                if mouse.column>l.end() && mouse.column<l.width && mouse.row>=3 && mouse.row<h-3 {
                    if mouse.row==3 && mouse.kind==MouseEventKind::Down(MouseButton::Left) && mouse.modifiers==KeyModifiers::NONE {
                        self.preview_mode=(self.preview_mode+1)%3;
                        self.preview_key=None;self.preview_scroll=0;
                        return Ok(None);
                    }
                    #[cfg(feature = "native-ops")]
                    if let Some(frame)=&self.visual_frame {
                        let page=match mouse.kind {MouseEventKind::ScrollDown=>self.visual_page.saturating_add(1).min(frame.pages),MouseEventKind::ScrollUp=>self.visual_page.saturating_sub(1).max(1),_=>self.visual_page};
                        if page!=self.visual_page {self.visual_page=page;self.preview_lines.clear();}
                        return Ok(None);
                    }
                    match mouse.kind {
                        MouseEventKind::ScrollDown=>self.preview_scroll=(self.preview_scroll+3).min(self.preview_lines.len().saturating_sub(l.capacity())),
                        MouseEventKind::ScrollUp=>self.preview_scroll=self.preview_scroll.saturating_sub(3),
                        _=>(),
                    }
                    return Ok(None);
                }
            }
        }
        if self.expanded_input(&event, model, l)? { return Ok(None); }
        if let Event::Mouse(mouse) = &mut event {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) && mouse.row == 0 {
                if let Some((i,_)) = MENUS.iter().enumerate().find(|(_, (name,x))| mouse.column >= *x && mouse.column < *x + name.len() as u16) {
                    self.side_focus = false;
                    let Some(selected) = self.menu(Some(i),writable)? else { return Ok(None); };
                    return self.input(selected,model,false,writable);
                }
                return Ok(None);
            }
            if self.dual {
                let width = w.saturating_sub(1);
                if h < 8 || mouse.row < 3 || mouse.row >= h - 3 || mouse.column >= width {
                    return Ok(None);
                }
                if width >= 54 {
                    let split = dual_split(width,self.prefs.dual_percent);
                    if mouse.column == split { return Ok(None); }
                    let right = mouse.column > split;
                    if right != self.active_right {
                        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left | MouseButton::Right)) {
                            // Focus first; never dispatch an action against rows that
                            // may change when the inactive pane is refreshed.
                            self.switch_pane(model)?;
                        }
                        return Ok(None);
                    }
                }
                if mouse.row == 3 { return Ok(None); }
                let index = model.top + usize::from(mouse.row - 4);
                match mouse.kind {
                    MouseEventKind::ScrollUp => model.shift(-1),
                    MouseEventKind::ScrollDown => model.shift(1),
                    MouseEventKind::Down(MouseButton::Left | MouseButton::Right) if index < model.entries.len() => {
                        model.selected = index;
                        if mouse.kind == MouseEventKind::Down(MouseButton::Right) && mouse.modifiers == KeyModifiers::NONE {
                            return self.context_menu(writable);
                        }
                        if writable && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                            && mouse.modifiers == KeyModifiers::CONTROL {
                            return Ok(Some(Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))));
                        }
                    }
                    _ => (),
                }
                return Ok(None);
            }
            if l.sidebar > 0 && mouse.column < l.sidebar && mouse.row >= 4 && h >= 8 && mouse.row < h-3 {
                self.side_focus = true;
                match mouse.kind {
                    MouseEventKind::ScrollUp => self.move_side(false),
                    MouseEventKind::ScrollDown => self.move_side(true),
                    MouseEventKind::Down(MouseButton::Left) => {
                        let i = self.side_top + usize::from(mouse.row-4);
                        if self.places.get(i).is_some_and(|p| p.1.is_some()) {
                            self.side_cursor = i;
                            self.open_side(model)?;
                        }
                    }
                    _ => (),
                }
                return Ok(None);
            }
            if l.file_row(mouse.column,mouse.row).is_some() {
                self.side_focus = false;
                let cells = Cells::new(l.end()-l.first(),l.capacity(),self.view,self.zoom);
                match mouse.kind {
                    MouseEventKind::ScrollUp => model.shift(-(cells.columns as isize)),
                    MouseEventKind::ScrollDown => model.shift(cells.columns as isize),
                    MouseEventKind::Down(MouseButton::Left | MouseButton::Right) => {
                        if let Some(offset) = cells.hit(mouse.column-l.first(),usize::from(mouse.row-4)) {
                            let index = model.top+offset;
                            if index < model.entries.len() {
                                model.selected = index;
                                if mouse.kind == MouseEventKind::Down(MouseButton::Right) && mouse.modifiers == KeyModifiers::NONE {
                                    return self.context_menu(writable);
                                }
                                if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                                    if writable && mouse.modifiers == KeyModifiers::CONTROL {
                                        return Ok(Some(Event::Key(KeyEvent::new(KeyCode::Char(' '),KeyModifiers::NONE))));
                                    }
                                    if mouse.modifiers == KeyModifiers::NONE && self.view == View::Tree && mouse.column < l.first()+4 {
                                        self.expand_current(model)?;
                                    }
                                }
                            }
                        }
                    }
                    _ => (),
                }
                return Ok(None);
            }
            return Ok(None);
        }
        if let Event::Key(key) = event {
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) { return Ok(None); }
            let code = key.code;
            // Modified keys must never accidentally dispatch a plain destructive shortcut.
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                let mapped = match code {
                    KeyCode::Char('q') => Some(KeyCode::Char('q')),
                    KeyCode::Char('c') if writable => Some(KeyCode::Char('c')),
                    KeyCode::Char('x') if writable => Some(KeyCode::Char('x')),
                    KeyCode::Char('v') if writable => Some(KeyCode::Char('v')),
                    KeyCode::Char('a') if writable => Some(KeyCode::Char('a')),
                    KeyCode::Char('z') if writable => Some(KeyCode::Char('u')),
                    KeyCode::Char('f') => Some(KeyCode::Char('/')),
                    KeyCode::Char('l') if cfg!(feature = "native-ops") => Some(KeyCode::Char('g')),
                    KeyCode::Char('c') | KeyCode::Char('p') => {
                        self.side_focus = false;
                        return Ok(Some(Event::Key(key)));
                    }
                    _ => None,
                };
                if let Some(code) = mapped {
                    self.side_focus = false;
                    return Ok(Some(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
                }
                return Ok(None);
            }
            if key.modifiers.contains(KeyModifiers::ALT) {
                let mapped = match code {
                    KeyCode::Left => Some(KeyCode::Char('[')),
                    KeyCode::Right => Some(KeyCode::Char(']')),
                    _ => None,
                };
                self.side_focus = false;
                return Ok(mapped.map(|code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
            }
            if code == KeyCode::F(10) {
                self.side_focus = false;
                let Some(selected) = self.menu(None,writable)? else { return Ok(None); };
                return self.input(selected,model,false,writable);
            }
            if code == KeyCode::F(8) && key.modifiers == KeyModifiers::NONE {
                if key.kind == KeyEventKind::Press { self.toggle_dual(model)?; }
                return Ok(None);
            }
            if code == KeyCode::F(8) && key.modifiers == KeyModifiers::SHIFT {
                self.side_focus = false;
                if cfg!(feature = "native-ops") {
                    return Ok(Some(Event::Key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE))));
                }
                return Ok(None);
            }
            if code == KeyCode::Tab || code == KeyCode::BackTab {
                if self.dual {
                    if key.kind == KeyEventKind::Press { self.switch_pane(model)?; }
                } else {
                    self.side_focus = l.sidebar > 0 && !self.side_focus;
                }
                return Ok(None);
            }
            if code == KeyCode::F(9) {
                if self.dual { self.dual = false; self.preview_enabled = true; }
                else { self.preview_enabled = !self.preview_enabled; }
                self.preview_key = None;
                return Ok(None);
            }
            if code == KeyCode::F(5) { self.preview_key = None; self.refresh_places(); }
            if self.side_focus {
                match code {
                    KeyCode::Up => self.move_side(false),
                    KeyCode::Down => self.move_side(true),
                    KeyCode::Enter | KeyCode::Right => self.open_side(model)?,
                    KeyCode::Esc | KeyCode::Left => self.side_focus = false,
                    KeyCode::Char('q') => return Ok(Some(Event::Key(key))),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(Some(Event::Key(key))),
                    _ => (),
                }
                return Ok(None);
            }
            if key.modifiers == KeyModifiers::NONE && code == KeyCode::Char('b') {
                if !self.bookmarks.contains(&model.directory) && self.bookmarks.len() < 64 {
                    self.bookmarks.push(model.directory.clone());
                    self.refresh_places();
                }
                return Ok(None);
            }
            if code == KeyCode::Char('B') {
                let labels: Vec<_> = self.bookmarks.iter().map(|p| display_name(p.as_os_str())).collect();
                if let Some(i) = super::choose("Session bookmarks / Saved locations",&labels)? { model.go(self.bookmarks[i].clone())?; }
                return Ok(None);
            }
            if !self.dual && key.modifiers == KeyModifiers::NONE {
                let cells = Cells::new(l.end()-l.first(),l.capacity(),self.view,self.zoom);
                match code {
                    KeyCode::Right if self.view == View::Tree => { self.expand_current(model)?; return Ok(None); }
                    KeyCode::Up if self.view == View::Grid => { model.shift(-(cells.columns as isize)); return Ok(None); }
                    KeyCode::Down if self.view == View::Grid => { model.shift(cells.columns as isize); return Ok(None); }
                    KeyCode::Left if self.view == View::Grid => { model.shift(-1); return Ok(None); }
                    KeyCode::Right if self.view == View::Grid => { model.shift(1); return Ok(None); }
                    KeyCode::PageUp => { model.shift(-(cells.capacity() as isize)); return Ok(None); }
                    KeyCode::PageDown => { model.shift(cells.capacity() as isize); return Ok(None); }
                    _ => (),
                }
            }
            let replacement = match code {
                KeyCode::F(4) if writable => Some(KeyCode::F(6)),
                KeyCode::Backspace => Some(KeyCode::Left),
                KeyCode::Null => return Ok(None),
                _ => None,
            };
            if let Some(code) = replacement { return Ok(Some(Event::Key(KeyEvent::new(code,KeyModifiers::NONE)))); }
            return Ok(Some(Event::Key(key)));
        }
        Ok(Some(event))
    }
}

fn draw_column(
    out: &mut impl Write,
    model: &mut Model,
    label: &str,
    active: bool,
    x: u16,
    width: u16,
    cap: usize,
    marked: &impl Fn(&std::path::Path) -> bool,
    icons: super::file_icons::Style,
    prefs: &super::app_settings::Preferences,
) -> io::Result<()> {
    let path = display_name(model.directory.as_os_str());
    let header = format!("{label}{} | {path}", if active { " *" } else { "" });
    queue!(out, MoveTo(x, 3), SetForegroundColor(if active { prefs.accent } else { prefs.foreground }),
        Print(super::clip(&header, usize::from(width))), SetForegroundColor(prefs.foreground), SetBackgroundColor(prefs.background))?;
    let (start, end) = model.viewport(cap);
    for (row, index) in (start..end).enumerate() {
        let entry = &model.entries[index];
        let (glyph, color) = prefs.icon(&entry.path,entry.directory,entry.symlink,icons);
        let text = format!("{} {} {}", if active && marked(&entry.path) { "[x]" } else { "   " }, glyph, entry.name);
        queue!(out, MoveTo(x, row as u16 + 4))?;
        if active && index == model.selected {
            queue!(out, SetBackgroundColor(prefs.selection))?;
        }
        queue!(out, SetForegroundColor(color), Print(super::clip(&text, usize::from(width))), SetForegroundColor(prefs.foreground), SetBackgroundColor(prefs.background))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn breadcrumb_and_sort_targets(){
        use std::path::Path;
        use terminal_tree::Order;
        let p=Path::new("/home/日本語/docs");
        assert_eq!(breadcrumb_hit(p,0,80),Some(PathBuf::from("/")));
        assert_eq!(breadcrumb_hit(p,4,80),Some(PathBuf::from("/home")));
        assert_eq!(breadcrumb_hit(p,9,80),None);
        assert_eq!(breadcrumb_hit(p,11,80),Some(PathBuf::from("/home/日本語")));
        assert_eq!(breadcrumb_hit(p,11,12),None);
        assert_eq!(list_sort_hit(40,60,true),Order::Size);
        assert_eq!(list_sort_hit(55,60,true),Order::Type);
        assert_eq!(list_sort_hit(32,40,false),Order::Size);
    }
    #[cfg(feature = "native-ops")]
    #[test]
    fn markdown_formats_without_loading_content(){
        let lines=["README.md","# Heading","- List","> Quote","<img src=x>"].map(str::to_owned);
        let (text,styles)=markdown_preview(&lines,80);
        assert_eq!(text[1],"Heading");
        assert_eq!(styles[1],PreviewStyle::Heading);
        assert_eq!(text[2],"• List");
        assert_eq!(styles[3],PreviewStyle::Quote);
        assert_eq!(text[4],"<img src=x>");
    }
    #[test]
    fn list_cells_fit_and_sizes_are_readable() {
        use unicode_width::UnicodeWidthStr;
        assert_eq!(human_size(1024),"1.0 KiB");
        assert_eq!(human_size(1048576),"1.0 MiB");
        for width in 0..150 {
            let text=list_columns("ภาษาไทย long name.txt","1.0 MiB","txt",width,true);
            assert!(text.width()<=usize::from(width));
        }
    }
    #[test]
    fn view_cells_and_toolbar_hit_testing() {
        assert_eq!(View::default(), View::Tree);
        for width in 0..150 {
            for height in 1..30 {
                for view in [View::Tree,View::Grid,View::List] {
                    for zoom in 1..=3 {
                        let c = Cells::new(width,height,view,zoom);
                        assert!(c.capacity() > 0);
                        assert!(c.groups*c.stride <= height);
                        for i in 0..c.capacity() {
                            let x = (i%c.columns) as u16*c.width;
                            let y = (i/c.columns)*c.stride;
                            if c.width > 0 { assert_eq!(c.hit(x,y),Some(i)); }
                        }
                    }
                }
            }
            if let Some(start) = toolbar_start(width) {
                assert_eq!(toolbar_hit(width,start),Some(0));
                assert_eq!(toolbar_hit(width,start+7),Some(1));
            }
        }
    }

    #[test]
    fn layout_is_responsive_and_only_file_rows_map_to_actions() {
        for width in 0..150 {
            for height in 0..30 {
                let l = Layout::new(width,height,true);
                assert!(l.first() <= l.end());
                assert!(l.end() <= l.width);
                assert!(l.file_row(0,0).is_none());
                if l.sidebar > 0 { assert!(l.file_row(0,4).is_none()); }
                if l.preview > 0 { assert!(l.file_row(l.end()+1,4).is_none()); }
                if height >= 8 && l.first() < l.end() {
                    assert_eq!(l.file_row(l.first(),4),Some(1));
                    assert!(l.file_row(l.first(),height-3).is_none());
                }
            }
        }
    }
}
