//! Opt-in X11 overlay transport. Core sends only validated RGB data to a
//! separately installed helper. A lifetime socket controls the child window.
use std::{io::{self,Read,Write},os::{fd::OwnedFd,unix::{net::UnixStream,fs::PermissionsExt}},path::Path,process::{Child,Command,Stdio},time::Duration};
use terminal_tree::visual_preview::Frame;
const HELPER:&str="/usr/local/bin/tt-x11-overlay";
#[derive(Clone,Copy)]
struct Geometry{window:u32,left:u16,top:u16,cw:u16,ch:u16}
fn geometry()->Option<Geometry>{
    if std::env::var("TT_GRAPHICS").as_deref()!=Ok("x11")||std::env::var("XDG_SESSION_TYPE").as_deref()!=Ok("x11")||std::env::var_os("DISPLAY").is_none()||std::env::var_os("TMUX").is_some()||std::env::var_os("STY").is_some(){return None;}
    if !std::fs::metadata(HELPER).is_ok_and(|m|m.is_file()&&m.permissions().mode()&0o111!=0){return None;}
    let s=std::env::var("TT_X11_WINDOW_ID").ok()?;
    let window=if let Some(hex)=s.strip_prefix("0x"){u32::from_str_radix(hex,16).ok()?}else{s.parse().ok()?};
    let v:Vec<u16>=std::env::var("TT_X11_GEOMETRY").ok()?.split(',').map(str::parse).collect::<Result<_,_>>().ok()?;
    let [left,top,cw,ch]:[u16;4]=v.try_into().ok()?;
    if window==0||left>128||top>256||!(4..=64).contains(&cw)||!(8..=128).contains(&ch){return None;}
    Some(Geometry{window,left,top,cw,ch})
}
pub fn supported()->bool{geometry().is_some()}
pub fn cell_size()->Option<(u32,u32)>{geometry().map(|g|(u32::from(g.cw),u32::from(g.ch)))}
pub struct Overlay{child:Child,socket:UnixStream}
impl Overlay{
    pub fn start(frame:&Frame,x:u16,y:u16,cols:u16,rows:u16)->io::Result<Self>{
        let g=geometry().ok_or_else(||io::Error::other("X11 overlay requires calibration"))?;
        let max_w=u32::from(cols)*u32::from(g.cw);
        let max_h=u32::from(rows)*u32::from(g.ch);
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
        let child=Command::new(Path::new(HELPER)).args([g.window.to_string(),left.to_string(),top.to_string(),width.to_string(),height.to_string()])
            .stdin(Stdio::from(stdin)).stdout(Stdio::from(stdout)).stderr(Stdio::null()).spawn()?;
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
