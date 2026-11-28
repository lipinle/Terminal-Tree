//! Bounded virtual terminal screen. Interpret selected controls as local
//! display data; never forward child escape sequences to the host.
use crossterm::{queue,style::{Attribute,Color,Print,SetAttribute,SetForegroundColor,SetBackgroundColor}};
use std::{io::{self,Write},ffi::OsStr,os::unix::ffi::OsStrExt};
use unicode_width::{UnicodeWidthChar,UnicodeWidthStr};
use std::collections::VecDeque;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
struct Style{fg:Option<Color>,bg:Option<Color>,bold:bool}
#[derive(Clone,Debug,Default)]
pub struct Line{spans:Vec<(Style,String)>}
impl Line{
    pub fn plain(text:String)->Self{Self{spans:vec![(Style::default(),text)]}}
    pub fn len(&self)->usize{self.spans.iter().map(|(_,s)|s.len()).sum()}
    #[cfg(test)]
    pub fn text(&self)->String{self.spans.iter().map(|(_,s)|s.as_str()).collect()}
    fn append_text(&mut self,style:Style,text:&str){
        if text.is_empty(){return;}
        if let Some((last,s))=self.spans.last_mut(){if *last==style{s.push_str(text);return;}}
        self.spans.push((style,text.to_owned()));
    }
    pub fn draw(&self,out:&mut impl Write,width:usize,fg:Color,bg:Color)->io::Result<()>{
        let mut room=width;
        for (style,text) in &self.spans{
            if room==0{break;}
            let clipped=super::clip(text,room);
            let cells=clipped.width();
            if clipped.is_empty()&&!text.is_empty(){break;}
            queue!(out,SetForegroundColor(style.fg.unwrap_or(fg)),SetBackgroundColor(style.bg.unwrap_or(bg)),
                SetAttribute(if style.bold{Attribute::Bold}else{Attribute::NormalIntensity}),Print(&clipped))?;
            room=room.saturating_sub(cells);
            if clipped.len()!=text.len(){break;}
        }
        queue!(out,SetAttribute(Attribute::NormalIntensity),SetForegroundColor(fg),SetBackgroundColor(bg))?;
        Ok(())
    }
}
fn apply(style:&mut Style,params:&[u8])->bool{
    let Ok(s)=std::str::from_utf8(params)else{return false;};
    let Some(nums)=s.split(';').map(|p|if p.is_empty(){Some(0)}else{p.parse::<u16>().ok()}).collect::<Option<Vec<_>>>()else{return false;};
    let mut next=*style;
    let mut i=0;
    while i<nums.len(){
        let n=nums[i];i+=1;
        match n{
            0=>next=Style::default(),1=>next.bold=true,22=>next.bold=false,
            30..=37=>next.fg=Some(Color::AnsiValue((n-30) as u8)),
            90..=97=>next.fg=Some(Color::AnsiValue((n-90+8) as u8)),
            40..=47=>next.bg=Some(Color::AnsiValue((n-40) as u8)),
            100..=107=>next.bg=Some(Color::AnsiValue((n-100+8) as u8)),
            39=>next.fg=None,49=>next.bg=None,
            38|48=>{
                let color=match nums.get(i){
                    Some(5) if nums.get(i+1).is_some_and(|v|*v<=255)=>{let c=Color::AnsiValue(nums[i+1] as u8);i+=2;c}
                    Some(2) if nums.get(i+1..i+4).is_some_and(|v|v.iter().all(|n|*n<=255))=>{
                        let c=Color::Rgb{r:nums[i+1] as u8,g:nums[i+2] as u8,b:nums[i+3] as u8};i+=4;c
                    }
                    _=>return false,
                };
                if n==38{next.fg=Some(color);}else{next.bg=Some(color);}
            }
            // Other SGR attributes are intentionally not rendered.
            _=>(),
        }
    }
    *style=next;true
}
// Fixed screen dimensions and bounded scrollback. A malicious CSI parameter
// cannot allocate rows or columns. Keep UTF-8 and CSI state between reads.
const MAX_COLS:u16=512;
const MAX_ROWS:u16=128;
const HISTORY_ROWS:usize=512;
const HISTORY_BYTES:usize=128*1024;
#[derive(Clone,Default)]
struct Cell {text:String,style:Style,wide:u8} // 0 blank, 1 normal, 2 lead, 3 continuation
#[derive(Default)]
enum Input {#[default] Ground,Escape,Csi(Vec<u8>),DiscardCsi,ControlString(bool),Utf8(Vec<u8>,usize)}
pub fn bounds(cols:u16,rows:u16)->(u16,u16){(cols.clamp(1,MAX_COLS),rows.clamp(1,MAX_ROWS))}
struct Primary {
    grid:Vec<Vec<Cell>>,x:usize,y:usize,saved:(usize,usize),style:Style,
    cursor_visible:bool,
}
pub struct Screen{
    cols:usize,rows:usize,grid:Vec<Vec<Cell>>,
    x:usize,y:usize,saved:(usize,usize),style:Style,input:Input,
    history:VecDeque<Line>,history_bytes:usize,
    primary:Option<Primary>,cursor_visible:bool,application_cursor:bool,
}
impl Screen{
    pub fn new(cols:u16,rows:u16)->Self{
        let (cols,rows)=bounds(cols,rows);
        let (cols,rows)=(usize::from(cols),usize::from(rows));
        Self{cols,rows,grid:vec![vec![Cell::default();cols];rows],x:0,y:0,saved:(0,0),style:Style::default(),input:Input::Ground,history:VecDeque::new(),history_bytes:0,primary:None,cursor_visible:true,application_cursor:false}
    }
    fn line(cells:&[Cell])->Line{
        let mut line=Line::default();
        let end=cells.iter().rposition(|c|c.wide!=0||c.style.bg.is_some()).map_or(0,|n|n+1);
        for c in &cells[..end]{
            if c.wide==3{continue;}
            if c.text.is_empty(){line.append_text(c.style," ");}else{line.append_text(c.style,&c.text);}
        }
        line
    }
    fn remember(&mut self,row:Vec<Cell>){
        let line=Self::line(&row);
        self.history_bytes+=line.len();
        self.history.push_back(line);
        while self.history.len()>HISTORY_ROWS||self.history_bytes>HISTORY_BYTES{
            if let Some(old)=self.history.pop_front(){self.history_bytes-=old.len();}else{break;}
        }
    }
    fn newline(&mut self){
        self.x=0;
        if self.y+1<self.rows{self.y+=1;}
        else{
            let row=self.grid.remove(0);if self.primary.is_none(){self.remember(row);}
            self.grid.push(vec![Cell::default();self.cols]);
        }
    }
    fn clear_cell(row:&mut [Cell],x:usize){
        if x>=row.len(){return;}
        match row[x].wide{
            2 if x+1<row.len()=>row[x+1]=Cell::default(),
            3 if x>0=>row[x-1]=Cell::default(),
            _=>(),
        }
        row[x]=Cell::default();
    }
    fn erase(&mut self,y:usize,start:usize,end:usize){
        let row=&mut self.grid[y];
        for x in start.min(self.cols)..end.min(self.cols){
            Self::clear_cell(row,x);
            row[x].style=self.style;
        }
    }
    fn printable(&mut self,c:char){
        let text=if c=='\\'{"\\".to_owned()}else{terminal_tree::display_name(OsStr::new(&c.to_string()))};
        // Escape invisible format/control characters into ordinary visible
        // characters; do not let them alter host text direction or controls.
        if text!=c.to_string(){for ch in text.chars(){self.put(ch);} }
        else{self.put(c);}
    }
    fn invalid(&mut self,bytes:&[u8]){
        for ch in terminal_tree::display_name(OsStr::from_bytes(bytes)).chars(){self.put(ch);}
    }
    fn put(&mut self,c:char){
        let width=c.width().unwrap_or(0);
        if width==0{
            if self.x>0{
                let row=&mut self.grid[self.y];
                let mut x=(self.x-1).min(self.cols-1);
                if row[x].wide==3&&x>0{x-=1;}
                if row[x].wide!=0&&row[x].text.len()+c.len_utf8()<=32{row[x].text.push(c);}
            }
            return;
        }
        if width>self.cols{return;}
        if self.x+width>self.cols{self.newline();}
        let row=&mut self.grid[self.y];
        Self::clear_cell(row,self.x);
        if width==2{Self::clear_cell(row,self.x+1);}
        row[self.x]=Cell{text:c.to_string(),style:self.style,wide:width as u8};
        if width==2{row[self.x+1]=Cell{text:String::new(),style:self.style,wide:3};}
        self.x+=width;
    }
    fn cursor(&mut self,x:usize,y:usize){self.x=x.min(self.cols-1);self.y=y.min(self.rows-1);}
    pub fn alternate(&self)->bool{self.primary.is_some()}
    pub fn application_cursor(&self)->bool{self.application_cursor}
    fn switch_screen(&mut self,on:bool){
        if on&&self.primary.is_none(){
            let grid=std::mem::replace(&mut self.grid,vec![vec![Cell::default();self.cols];self.rows]);
            self.primary=Some(Primary{grid,x:self.x,y:self.y,saved:self.saved,style:self.style,cursor_visible:self.cursor_visible});
            self.x=0;self.y=0;self.saved=(0,0);self.style=Style::default();
        }else if !on{
            if let Some(p)=self.primary.take(){
                self.grid=p.grid;self.saved=p.saved;self.style=p.style;self.cursor_visible=p.cursor_visible;
                self.cursor(p.x,p.y);
            }
        }
    }
    fn csi(&mut self,params:&[u8],cmd:u8){
        if matches!(cmd,b'h'|b'l')&&params.first()==Some(&b'?'){
            if let Ok(s)=std::str::from_utf8(&params[1..]){
                for mode in s.split(';'){
                    let on=cmd==b'h';
                    match mode{
                        "1"=>self.application_cursor=on,
                        "25"=>self.cursor_visible=on,
                        // Treat all three alternate-buffer modes as a fresh
                        // isolated buffer; never mix alternate output into history.
                        "47"|"1047"|"1049"=>self.switch_screen(on),
                        _=>(),
                    }
                }
            }
            return;
        }
        if cmd==b'm'{let _=apply(&mut self.style,params);return;}
        let Ok(s)=std::str::from_utf8(params)else{return;};
        let Some(n)=s.split(';').map(|p|if p.is_empty(){Some(0)}else{p.parse::<u16>().ok().map(usize::from)}).collect::<Option<Vec<_>>>()else{return;};
        let raw=n.first().copied().unwrap_or(0);
        let a=raw.max(1);
        let b=n.get(1).copied().unwrap_or(0).max(1);
        match cmd{
            b'A'=>self.cursor(self.x,self.y.saturating_sub(a)),
            b'B'=>self.cursor(self.x,self.y.saturating_add(a)),
            b'C'=>self.cursor(self.x.saturating_add(a),self.y),
            b'D'=>self.cursor(self.x.saturating_sub(a),self.y),
            b'E'=>self.cursor(0,self.y.saturating_add(a)),
            b'F'=>self.cursor(0,self.y.saturating_sub(a)),
            b'G'|b'\x60'=>self.cursor(a-1,self.y),
            b'd'=>self.cursor(self.x,a-1),
            b'H'|b'f'=>self.cursor(b-1,a-1),
            b's'=>self.saved=(self.x,self.y),
            b'u'=>self.cursor(self.saved.0,self.saved.1),
            b'K'=>match raw{0=>self.erase(self.y,self.x,self.cols),1=>self.erase(self.y,0,self.x+1),2=>self.erase(self.y,0,self.cols),_=>()},
            b'J'=>match raw{
                0=>{self.erase(self.y,self.x,self.cols);for y in self.y+1..self.rows{self.erase(y,0,self.cols);}},
                1=>{for y in 0..self.y{self.erase(y,0,self.cols);}self.erase(self.y,0,self.x+1);},
                2=>{for y in 0..self.rows{self.erase(y,0,self.cols);}},
                _=>(), // never erase scrollback on behalf of a child
            },
            _=>(),
        }
    }
    fn byte(&mut self,b:u8){
        match std::mem::take(&mut self.input){
            Input::Ground=>match b{
                27=>self.input=Input::Escape,
                b'\r'=>self.x=0,b'\n'=>self.newline(),
                8=>self.x=self.x.saturating_sub(1),
                b'\t'=>self.x=((self.x/8+1)*8).min(self.cols-1),
                32..=126=>self.put(char::from(b)),
                0xc2..=0xf4=>{let n=if b<0xe0{2}else if b<0xf0{3}else{4};self.input=Input::Utf8(vec![b],n);},
                0x80..=0xff=>self.invalid(&[b]),
                _=>(), // BEL, NUL and other C0 controls have no host effect
            },
            Input::Escape=>match b{
                b'['=>self.input=Input::Csi(Vec::new()),
                b']'|b'P'|b'_'|b'^'|b'X'=>self.input=Input::ControlString(false),
                b'7'=>self.saved=(self.x,self.y),
                b'8'=>self.cursor(self.saved.0,self.saved.1),
                27=>self.input=Input::Escape,
                _=>(),
            },
            Input::Csi(mut p)=>{
                if (0x40..=0x7e).contains(&b){self.csi(&p,b);}
                else if b==27{self.input=Input::Escape;}
                else if p.len()<96&&(0x20..=0x3f).contains(&b){p.push(b);self.input=Input::Csi(p);}
                else{self.input=Input::DiscardCsi;}
            }
            Input::DiscardCsi=>{if b==27{self.input=Input::Escape;}else if !(0x40..=0x7e).contains(&b){self.input=Input::DiscardCsi;}}
            Input::ControlString(esc)=>{
                // OSC/DCS/APC/PM/SOS are ignored, even across read chunks.
                if b!=7&&!(esc&&b==b'\\'){self.input=Input::ControlString(b==27);}
            }
            Input::Utf8(mut bytes,n)=>{
                if (0x80..=0xbf).contains(&b){
                    bytes.push(b);
                    if bytes.len()==n{
                        match std::str::from_utf8(&bytes){Ok(s)=>{for c in s.chars(){self.printable(c);}},Err(_)=>self.invalid(&bytes)}
                    }else{self.input=Input::Utf8(bytes,n);}
                }else{self.invalid(&bytes);self.byte(b);}
            }
        }
    }
    pub fn feed(&mut self,bytes:&[u8]){for &b in bytes{self.byte(b);}}
    pub fn resize(&mut self,cols:u16,rows:u16){
        let (cols,rows)=bounds(cols,rows);let (cols,rows)=(usize::from(cols),usize::from(rows));
        while self.grid.len()>rows{
            if self.y>=rows{let row=self.grid.remove(0);if self.primary.is_none(){self.remember(row);}self.y-=1;}
            else{self.grid.pop();}
        }
        for row in &mut self.grid{
            row.resize(cols,Cell::default());
            if row.last().is_some_and(|c|c.wide==2){row[cols-1]=Cell::default();}
        }
        self.grid.resize_with(rows,||vec![Cell::default();cols]);
        if let Some(p)=&mut self.primary{
            while p.grid.len()>rows{if p.y>=rows{p.grid.remove(0);p.y-=1;}else{p.grid.pop();}}
            for row in &mut p.grid{row.resize(cols,Cell::default());if row.last().is_some_and(|c|c.wide==2){row[cols-1]=Cell::default();}}
            p.grid.resize_with(rows,||vec![Cell::default();cols]);
            p.x=p.x.min(cols-1);p.y=p.y.min(rows-1);
        }
        self.cols=cols;self.rows=rows;self.cursor(self.x,self.y);
    }
    pub fn lines(&self)->Vec<Line>{self.display_lines(None)}
    pub fn display_lines(&self,cursor:Option<(Color,Color)>)->Vec<Line>{
        let mut lines:Vec<_>=if self.alternate(){Vec::new()}else{self.history.iter().cloned().collect()};
        let last=if self.alternate(){self.rows-1}else{self.grid.iter().rposition(|row|row.iter().any(|c|c.wide!=0||c.style.bg.is_some())).unwrap_or(0).max(self.y)};
        for (y,row) in self.grid[..=last].iter().enumerate(){
            if let Some((fg,bg))=cursor.filter(|_|self.cursor_visible&&self.y==y){
                let mut row=row.clone();
                let mut x=self.x.min(self.cols-1);
                if row[x].wide==3&&x>0{x-=1;}
                row[x].style.fg=Some(fg);row[x].style.bg=Some(bg);
                if row[x].wide==0{row[x].wide=1;row[x].text=" ".into();}
                lines.push(Self::line(&row));
            }else{lines.push(Self::line(row));}
        }
        lines
    }
}
pub fn parse(bytes:&[u8])->Vec<Line>{
    // Command console has no interactive tty width. Use the same bounded
    // virtual screen to support carriage-return progress and ANSI colors.
    let mut screen=Screen::new(512,32);
    screen.feed(bytes);
    screen.lines()
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn sgr_is_data_not_host_control(){
        let lines=parse(b"\x1b[1;31mred\nstill red\x1b[0m plain \x1b]52;c;payload\x07\n\x1b[38;2;1;2;3mRGB");
        assert_eq!(lines[0].text(),"red");
        assert_eq!(lines[0].spans[0].0.fg,Some(Color::AnsiValue(1)));
        assert!(lines[0].spans[0].0.bold);
        assert_eq!(lines[1].spans[0].0,lines[0].spans[0].0);
        assert!(lines[1].text().contains("plain"));
        assert!(!lines[1].text().contains('\x1b'));
        assert_eq!(lines[2].spans[0].0.fg,Some(Color::Rgb{r:1,g:2,b:3}));
        let mut output=Vec::new();lines[1].draw(&mut output,80,Color::White,Color::Black).unwrap();
        assert!(!output.windows(4).any(|w|w==b"\x1b]52"));
        assert_eq!(parse(b"\x1b[38;2;999;0;0mX")[0].spans[0].0,Style::default());
    }
    #[test]fn progress_cursor_erase_and_chunked_input(){
        let mut s=Screen::new(20,4);
        s.feed(b"progress 10%\rprogress 100%\x1b[K\nsecond");
        assert_eq!(s.lines()[0].text(),"progress 100%");
        s.feed(b"\x1b[1;1Hdone\x1b[K\x1b[2;");
        s.feed(b"1Hupdated\x1b[K");
        assert_eq!(s.lines()[0].text(),"done");
        assert_eq!(s.lines()[1].text(),"updated");
        s.feed(b"\x1b]52;c;ignored");s.feed(b"\x1b\\");
        assert_eq!(s.lines()[1].text(),"updated");
        s.feed(b"\n");let thai="ไทย".as_bytes();s.feed(&thai[..2]);s.feed(&thai[2..]);
        assert_eq!(s.lines()[2].text(),"ไทย");
        s.feed(b"\x1b[65535;65535H!");
        assert_eq!(s.x,20);assert_eq!(s.y,3);
        s.resize(8,2);assert_eq!((s.cols,s.rows),(8,2));
    }
    #[test]fn scrollback_and_wide_overwrite_are_bounded(){
        let mut s=Screen::new(8,2);
        s.feed("日A\rX".as_bytes());
        assert_eq!(s.lines()[0].text(),"X A");
        for _ in 0..800{s.feed(b"\nline");}
        assert!(s.history.len()<=HISTORY_ROWS);
        assert!(s.history_bytes<=HISTORY_BYTES);
    }
    #[test]fn alternate_isolated_and_cursor_local(){
        let mut s=Screen::new(16,4);
        s.feed(b"primary\x1b[?1049h\x1b[?1h\x1b[?25lALT");
        assert!(s.alternate()&&s.application_cursor()&&!s.cursor_visible);
        assert_eq!(s.lines().len(),4);
        assert_eq!(s.lines()[0].text(),"ALT");
        assert!(s.display_lines(Some((Color::Black,Color::White)))[0].spans.iter().all(|(st,_)|st.bg.is_none()));
        s.feed(b"\n\n\n\nnew");
        assert!(s.history.is_empty());
        s.resize(12,3);
        s.feed(b"\x1b[?1049l\x1b[?1l");
        assert!(!s.alternate()&&!s.application_cursor()&&s.cursor_visible);
        assert_eq!(s.lines()[0].text(),"primary");
        let line=s.display_lines(Some((Color::Black,Color::White)));
        assert!(line[0].spans.iter().any(|(st,_)|st.bg==Some(Color::White)));
        assert_eq!(s.lines()[0].text(),"primary");
    }
    #[test]fn malformed_and_wide_text_are_bounded(){
        let lines=parse("ไทย\x1b[32m日\x1b[0mZ".as_bytes());let mut out=Vec::new();
        lines[0].draw(&mut out,2,Color::White,Color::Black).unwrap();
        assert!(!String::from_utf8(out).unwrap().contains("Z"));
    }
}
