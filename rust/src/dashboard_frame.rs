//! Assemble trusted dashboard drawing commands in memory. Paint complete rows
//! instead of erasing the whole screen, including on terminals without sync.
use crossterm::{cursor::MoveTo,queue,style::{Color,ResetColor,SetBackgroundColor},terminal::{Clear,ClearType}};
use std::io::{self,Write};

#[derive(Default)]
pub struct FrameCache { last: Option<(u16,u16,Vec<u8>)> }
impl FrameCache {
    pub fn invalidate(&mut self){self.last=None;}
    pub fn present(&mut self,frame:Vec<u8>,width:u16,height:u16,background:Color)->io::Result<()>{
        if self.last.as_ref().is_some_and(|(w,h,old)|*w==width&&*h==height&&old==&frame){return Ok(());}
        #[cfg(target_os="linux")]
        let output=compose(&frame,height,background)?;
        #[cfg(not(target_os="linux"))]
        let output={let _=background;frame.clone()};
        let mut out=io::stdout().lock();
        out.write_all(&output)?;
        out.flush()?;
        self.last=Some((width,height,frame));
        Ok(())
    }
}

#[cfg(target_os="linux")]
fn compose(frame:&[u8],height:u16,background:Color)->io::Result<Vec<u8>>{
    let invalid=||io::Error::other("unexpected dashboard drawing command");
    let mut rows=vec![Vec::new();usize::from(height)];
    let mut row=None;
    let(mut fg,mut bg,mut intensity)=(Vec::new(),Vec::new(),Vec::new());
    let mut i=0;
    while i<frame.len(){
        if frame[i]==27{
            let start=i;
            if frame.get(i+1)!=Some(&b'['){return Err(invalid());}
            i+=2;
            let arg=i;
            while i<frame.len()&&(0x30..=0x3f).contains(&frame[i]){i+=1;}
            let cmd=*frame.get(i).ok_or_else(invalid)?;
            let params=&frame[arg..i];i+=1;
            let seq=&frame[start..i];
            match cmd{
                b'H'=>{
                    let s=std::str::from_utf8(params).map_err(|_|invalid())?;
                    let (y,x)=s.split_once(';').ok_or_else(invalid)?;
                    let y=y.parse::<usize>().map_err(|_|invalid())?;
                    let x=x.parse::<usize>().map_err(|_|invalid())?;
                    if y==0||x==0{return Err(invalid());}
                    row=(y<=rows.len()).then_some(y-1);
                    if let Some(n)=row {
                        let r=&mut rows[n];r.extend_from_slice(seq);
                        r.extend_from_slice(b"\x1b[0m");
                        r.extend_from_slice(&fg);r.extend_from_slice(&bg);r.extend_from_slice(&intensity);
                    }
                }
                b'm'=>{
                    let first=params.split(|&b|b==b';').next().unwrap_or_default();
                    match first{
                        b""|b"0"=>{fg.clear();bg.clear();intensity.clear();}
                        b"38"|b"39"=>{fg=seq.to_vec();}
                        b"48"|b"49"=>{bg=seq.to_vec();}
                        b"1"|b"22"=>{intensity=seq.to_vec();}
                        _=>return Err(invalid()),
                    }
                    if let Some(n)=row{rows[n].extend_from_slice(seq);}
                }
                b'J' if params==b"2"=>(), // logical clear; never sent to terminal
                _=>return Err(invalid()),
            }
        }else{
            let start=i;
            while i<frame.len()&&frame[i]!=27{
                if frame[i]<32||frame[i]==127{return Err(invalid());}
                i+=1;
            }
            if let Some(n)=row{rows[n].extend_from_slice(&frame[start..i]);}
        }
    }
    let mut out=Vec::new();
    // Terminals without DEC synchronized-update support ignore these modes.
    out.extend_from_slice(b"\x1b[?2026h");
    for (y,row) in rows.into_iter().enumerate(){
        queue!(out,MoveTo(0,y as u16),ResetColor,SetBackgroundColor(background),Clear(ClearType::CurrentLine))?;
        out.extend(row);
    }
    queue!(out,ResetColor)?;
    out.extend_from_slice(b"\x1b[?2026l");
    Ok(out)
}
#[cfg(all(test,target_os="linux"))]
mod tests{
    use super::*;
    #[test]fn no_full_clear_and_styles_survive_row_switches(){
        let frame=b"\x1b[1;1H\x1b[38;5;9m\x1b[2Jone\x1b[2;1Htwo\x1b[1;5Hthree";
        let out=compose(frame,3,Color::Reset).unwrap();
        assert!(!out.windows(4).any(|w|w==b"\x1b[2J"));
        assert_eq!(out.windows(4).filter(|w|*w==b"\x1b[2K").count(),3);
        assert!(out.windows(5).any(|w|w==b"three"));
        assert!(out.windows(9).filter(|w|*w==b"\x1b[38;5;9m").count()>=3);
        assert!(compose(b"\x1b]0;bad\x07",3,Color::Reset).is_err());
    }
}
