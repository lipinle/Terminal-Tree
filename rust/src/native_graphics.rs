//! Kitty direct RGB graphics. Only locally encoded, validated pixel data.
//! No terminal queries, no filenames, no raw helper control sequences.
use crossterm::{cursor::MoveTo,queue};
use std::io::{self,Write};
use terminal_tree::visual_preview::Frame;
pub fn supported()->bool{
    if std::env::var_os("TMUX").is_some()||std::env::var_os("STY").is_some(){return false;}
    match std::env::var("TT_GRAPHICS").as_deref(){
        Ok("kitty")=>true,Ok("unicode")=>false,
        Ok("x11")=>super::x11_overlay::supported(),
        _=>std::env::var("TERM").as_deref()==Ok("xterm-kitty")&&std::env::var_os("KITTY_WINDOW_ID").is_some(),
    }
}
pub fn pixel_bounds(cols:u16,rows:u16)->(u32,u32){
    // Actual window pixel dimensions when available; conservative estimate otherwise.
    let (cw,ch)=super::x11_overlay::cell_size().unwrap_or_else(||crossterm::terminal::window_size().ok().filter(|s|s.width>0&&s.height>0&&s.columns>0&&s.rows>0)
        .map(|s|((u32::from(s.width)/u32::from(s.columns)).clamp(1,64),(u32::from(s.height)/u32::from(s.rows)).clamp(1,128))).unwrap_or((10,20)));
    ((u32::from(cols)*cw).clamp(1,1024),(u32::from(rows)*ch).clamp(1,1024))
}
fn b64(input:&[u8])->Vec<u8>{
    const A:&[u8;64]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out=Vec::with_capacity((input.len()+2)/3*4);
    for b in input.chunks(3){
        let a=b[0];let c=*b.get(1).unwrap_or(&0);let d=*b.get(2).unwrap_or(&0);
        out.extend([A[(a>>2) as usize],A[((a&3)<<4|c>>4) as usize],
            if b.len()>1{A[((c&15)<<2|d>>6) as usize]}else{b'='},
            if b.len()>2{A[(d&63) as usize]}else{b'='}]);
    }
    out
}
fn encode(out:&mut impl Write,id:u32,w:usize,h:usize,pixels:&[u8],cols:u16,rows:u16)->io::Result<()>{
    if w==0||h==0||w>1024||h>1024||pixels.len()!=w*h*3||cols==0||rows==0{return Err(io::Error::other("invalid native graphics frame"));}
    // 3072 raw bytes => at most 4096 base64 bytes, no padding except final chunk.
    let total=(pixels.len()+3071)/3072;
    for (i,chunk) in pixels.chunks(3072).enumerate(){
        let more=u8::from(i+1<total);
        if i==0{write!(out,"\x1b_Ga=T,t=d,f=24,s={w},v={h},i={id},c={cols},r={rows},C=1,q=2,m={more};")?;}
        else{write!(out,"\x1b_Gq=2,m={more};")?;}
        out.write_all(&b64(chunk))?;out.write_all(b"\x1b\\")?;
    }
    Ok(())
}
pub fn fit(frame:&Frame,cols:u16,rows:u16)->(u16,u16){
    let (w,h)=pixel_bounds(cols,rows);
    // pixel_bounds may cap the requested size, so use the relative cell ratio.
    let scale=(f64::from(w)/frame.width as f64).min(f64::from(h)/frame.height as f64);
    let c=((frame.width as f64*scale/f64::from(w))*f64::from(cols)).round() as u16;
    let r=((frame.height as f64*scale/f64::from(h))*f64::from(rows)).round() as u16;
    (c.clamp(1,cols.max(1)),r.clamp(1,rows.max(1)))
}
pub struct Slot {id:u32,visible:bool,overlay:Option<super::x11_overlay::Overlay>}
impl Slot{
    pub fn new(salt:u32)->Self{Self{id:(std::process::id().wrapping_mul(2654435761)^salt).max(1),visible:false,overlay:None}}
    pub fn clear(&mut self)->io::Result<()>{
        self.overlay=None;
        if self.visible{
            let mut out=io::stdout().lock();
            write!(out,"\x1b_Ga=d,d=I,i={},q=2;\x1b\\",self.id)?;out.flush()?;self.visible=false;
        }
        Ok(())
    }
    pub fn overlay_expired(&mut self)->bool{if let Some(o)=&mut self.overlay{if !o.alive(){self.overlay=None;return true;}}false}
    pub fn show(&mut self,frame:&Frame,x:u16,y:u16,cols:u16,rows:u16)->io::Result<()>{
        self.clear()?;
        if super::x11_overlay::requested(){self.overlay=Some(super::x11_overlay::Overlay::start(frame,x,y,cols,rows)?);return Ok(());}
        let mut buf=Vec::new();queue!(buf,MoveTo(x,y))?;
        encode(&mut buf,self.id,frame.width,frame.height,frame.rgb(),cols,rows)?;
        self.visible=true; // cleanup even on a partial write
        let mut out=io::stdout().lock();out.write_all(&buf)?;out.flush()
    }
}
impl Drop for Slot{fn drop(&mut self){let _=self.clear();}}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn base64_and_chunk_bounds(){
        assert_eq!(b64(b"f"),b"Zg==");assert_eq!(b64(b"foo"),b"Zm9v");
        let mut out=Vec::new();encode(&mut out,42,40,40,&vec![0;4800],4,4).unwrap();
        let s=std::str::from_utf8(&out).unwrap();
        let chunks:Vec<_>=s.split("\x1b\\").filter(|s|!s.is_empty()).collect();
        assert_eq!(chunks.len(),2);
        assert!(chunks[0].contains("m=1;"));assert!(chunks[1].contains("m=0;"));
        assert!(chunks.iter().all(|s|s.split_once(';').unwrap().1.len()<=4096));
        assert!(encode(&mut out,1,1025,1,&[],1,1).is_err());
    }
}
