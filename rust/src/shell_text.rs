//! Interpret only SGR color/intensity as display data, never forward child
//! escape sequences to the host. Non-SGR controls remain escaped.
use crossterm::{queue,style::{Attribute,Color,Print,SetAttribute,SetForegroundColor,SetBackgroundColor}};
use std::{io::{self,Write},ffi::OsStr,os::unix::ffi::OsStrExt};
use unicode_width::UnicodeWidthStr;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
struct Style{fg:Option<Color>,bg:Option<Color>,bold:bool}
#[derive(Clone,Debug,Default)]
pub struct Line{spans:Vec<(Style,String)>}
impl Line{
    pub fn plain(text:String)->Self{Self{spans:vec![(Style::default(),text)]}}
    pub fn len(&self)->usize{self.spans.iter().map(|(_,s)|s.len()).sum()}
    #[cfg(test)]
    pub fn text(&self)->String{self.spans.iter().map(|(_,s)|s.as_str()).collect()}
    fn append(&mut self,style:Style,bytes:&[u8]){
        if bytes.is_empty(){return;}
        let text=terminal_tree::display_name(OsStr::from_bytes(bytes));
        if let Some((last,s))=self.spans.last_mut(){if *last==style{s.push_str(&text);return;}}
        self.spans.push((style,text));
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
pub fn parse(bytes:&[u8])->Vec<Line>{
    let mut lines=Vec::new();
    let mut line=Line::default();
    let mut style=Style::default();
    let(mut i,mut start)=(0,0);
    while i<bytes.len(){
        if bytes[i]==b'\n'{
            let end=if i>start&&bytes[i-1]==b'\r'{i-1}else{i};
            line.append(style,&bytes[start..end]);lines.push(std::mem::take(&mut line));
            i+=1;start=i;continue;
        }
        if bytes[i]==27&&bytes.get(i+1)==Some(&b'['){
            let mut end=i+2;
            // Cap individual sequences. No cursor movement, OSC, DCS or
            // terminal mode controls are ever executed or re-emitted.
            while end<bytes.len()&&end-i<=96&&(bytes[end].is_ascii_digit()||bytes[end]==b';'){end+=1;}
            if bytes.get(end)==Some(&b'm')&&end-i<=96{
                let old=style;
                if apply(&mut style,&bytes[i+2..end]){
                    line.append(old,&bytes[start..i]);i=end+1;start=i;continue;
                }
            }
        }
        i+=1;
    }
    line.append(style,&bytes[start..]);
    if !line.spans.is_empty()||bytes.is_empty(){lines.push(line);}
    lines
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
    #[test]fn malformed_and_wide_text_are_bounded(){
        let lines=parse("ไทย\x1b[32m日\x1b[0mZ".as_bytes());let mut out=Vec::new();
        lines[0].draw(&mut out,2,Color::White,Color::Black).unwrap();
        assert!(!String::from_utf8(out).unwrap().contains("Z"));
    }
}
