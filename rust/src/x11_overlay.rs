//! Opt-in X11 overlay transport. Core sends only validated RGB data to a
//! separately installed helper. A lifetime socket controls the child window.
use std::{io::{self,Read,Write},os::{fd::OwnedFd,unix::{net::UnixStream,fs::PermissionsExt}},path::Path,process::{Child,Command,Stdio},time::Duration,sync::{Mutex,OnceLock}};
use terminal_tree::visual_preview::Frame;
const HELPER:&str="/usr/local/bin/tt-x11-overlay";
#[derive(Clone,Copy)]
struct Geometry{window:u32,left:u16,top:u16,cw:u16,ch:u16,trim_w:u16,trim_h:u16,client:Option<(u16,u16)>}
#[derive(Default)]
struct ProbeCache{attempted:bool,pinned:Option<u32>,size:Option<(u16,u16)>,value:Option<Geometry>}
static CACHE:OnceLock<Mutex<ProbeCache>>=OnceLock::new();
pub fn requested()->bool{std::env::var("TT_GRAPHICS").as_deref()==Ok("x11")}
pub fn installed()->bool{std::fs::metadata(HELPER).is_ok_and(|m|m.is_file()&&m.permissions().mode()&0o111!=0)}
fn window_id(s:&str)->Option<u32>{if let Some(hex)=s.strip_prefix("0x"){u32::from_str_radix(hex,16).ok()}else{s.parse().ok()}}
fn parse_probe(text:&str)->Option<Geometry>{
    let v:Vec<u32>=text.split_ascii_whitespace().map(str::parse).collect::<Result<_,_>>().ok()?;
    let [window,left,top,cw,ch,w,h]:[u32;7]=v.try_into().ok()?;
    if window==0||left>128||top>256||!(4..=64).contains(&cw)||!(8..=128).contains(&ch)||w==0||h==0||w>65535||h>65535{return None;}
    Some(Geometry{window,left:left as u16,top:top as u16,cw:cw as u16,ch:ch as u16,trim_w:left as u16,trim_h:top as u16,client:Some((w as u16,h as u16))})
}
fn geometry()->Option<Geometry>{
    if !requested()||std::env::var("XDG_SESSION_TYPE").as_deref()!=Ok("x11")||std::env::var_os("DISPLAY").is_none()||std::env::var_os("TMUX").is_some()||std::env::var_os("STY").is_some()||std::env::var_os("SSH_CONNECTION").is_some()||!installed(){return None;}
    if let Some(raw)=std::env::var_os("TT_X11_GEOMETRY"){
        let window=window_id(&std::env::var("TT_X11_WINDOW_ID").ok()?)?;
        let v:Vec<u16>=raw.to_str()?.split(',').map(str::parse).collect::<Result<_,_>>().ok()?;
        let [left,top,cw,ch]:[u16;4]=v.try_into().ok()?;
        if window==0||left>128||top>256||!(4..=64).contains(&cw)||!(8..=128).contains(&ch){return None;}
        return Some(Geometry{window,left,top,cw,ch,trim_w:0,trim_h:0,client:None});
    }
    let size=crossterm::terminal::size().ok()?;
    let mut cache=CACHE.get_or_init(||Mutex::new(ProbeCache::default())).lock().ok()?;
    if cache.size==Some(size){return cache.value;}
    // Never bind an unrelated active window after initial failure or focus loss.
    if cache.attempted&&cache.pinned.is_none(){return None;}
    let explicit=std::env::var("TT_X11_WINDOW_ID").ok();
    let pinned=match explicit{Some(s)=>Some(window_id(&s)?),None=>cache.pinned};
    cache.attempted=true;cache.size=Some(size);cache.value=None;
    let mut cmd=Command::new("/usr/bin/timeout");
    cmd.args(["--kill-after=1s","1s",HELPER,"--probe"]).args([size.0.to_string(),size.1.to_string()]);
    if let Some(id)=pinned{cmd.arg(id.to_string());}
    let output=cmd.stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !output.status.success()||output.stdout.len()>128{return None;}
    let g=parse_probe(std::str::from_utf8(&output.stdout).ok()?)?;
    if pinned.is_some_and(|id|id!=g.window){return None;}
    cache.pinned=Some(g.window);cache.value=Some(g);Some(g)
}
pub fn supported()->bool{geometry().is_some()}
pub fn cell_size()->Option<(u32,u32)>{geometry().map(|g|(u32::from(g.cw),u32::from(g.ch)))}
pub fn refresh(){
    if let Some(cache)=CACHE.get(){if let Ok(mut c)=cache.lock(){if c.pinned.is_some(){c.size=None;}}}
}
pub struct Overlay{child:Child,socket:UnixStream}
impl Overlay{
    pub fn start(frame:&Frame,x:u16,y:u16,cols:u16,rows:u16)->io::Result<Self>{
        let g=geometry().ok_or_else(||io::Error::other("X11 overlay requires calibration"))?;
        let max_w=(u32::from(cols)*u32::from(g.cw)).saturating_sub(u32::from(g.trim_w));
        let max_h=(u32::from(rows)*u32::from(g.ch)).saturating_sub(u32::from(g.trim_h));
        if frame.width==0||frame.height==0||frame.width>1024||frame.height>1024||max_w==0||max_h==0{return Err(io::Error::other("invalid overlay size"));}
        let scale=(f64::from(max_w)/frame.width as f64).min(f64::from(max_h)/frame.height as f64).min(1.0);
        let width=(frame.width as f64*scale).floor().max(1.0) as usize;
        let height=(frame.height as f64*scale).floor().max(1.0) as usize;
        let left=u32::from(g.left)+u32::from(x)*u32::from(g.cw)+(max_w-width as u32)/2;
        let top=u32::from(g.top)+u32::from(y)*u32::from(g.ch);
        if left>i16::MAX as u32||top>i16::MAX as u32{return Err(io::Error::other("overlay position out of bounds"));}
        let mut pixels=Vec::with_capacity(width*height*3);
        for py in 0..height{for px in 0..width{pixels.extend(frame.pixel(px*frame.width/width,py*frame.height/height));}}
        let (mut socket,other)=UnixStream::pair()?;
        socket.set_write_timeout(Some(Duration::from_millis(750)))?;
        socket.set_read_timeout(Some(Duration::from_millis(750)))?;
        let stdin:OwnedFd=other.try_clone()?.into();
        let stdout:OwnedFd=other.into();
        let mut command=Command::new(Path::new(HELPER));
        command.args([g.window.to_string(),left.to_string(),top.to_string(),width.to_string(),height.to_string()]);
        if let Some((w,h))=g.client{command.args([w.to_string(),h.to_string()]);}
        let child=command.stdin(Stdio::from(stdin)).stdout(Stdio::from(stdout)).stderr(Stdio::null()).spawn()?;
        let mut overlay=Self{child,socket:socket.try_clone()?};
        let result=(||{socket.write_all(&pixels)?;let mut ack=[0;3];socket.read_exact(&mut ack)?;
            if &ack!=b"OK\n"{return Err(io::Error::other("X11 helper rejected placement"));}
            Ok(())})();
        if let Err(e)=result{overlay.stop();return Err(e);}
        Ok(overlay)
    }
    pub fn alive(&mut self)->bool{matches!(self.child.try_wait(),Ok(None))}
    fn stop(&mut self){
        let _=self.socket.shutdown(std::net::Shutdown::Both);
        if self.alive(){let _=self.child.kill();let _=self.child.wait();}
    }
}
impl Drop for Overlay{fn drop(&mut self){self.stop();}}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn strict_probe(){
        let g=parse_probe("42 4 30 10 20 804 530").unwrap();
        assert_eq!((g.window,g.trim_w,g.trim_h,g.client),(42,4,30,Some((804,530))));
        assert!(parse_probe("42 129 30 10 20 804 530").is_none());
        assert!(parse_probe("42 4 30 10 20 804 530 extra").is_none());
    }
}
