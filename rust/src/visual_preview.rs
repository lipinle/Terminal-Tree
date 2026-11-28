//! Optional pixel-only subprocess protocol. No shell or ANSI input.
use crate::readonly_file::ReadSession;
use std::{io::{self,Read},path::Path,process::{Command,Stdio},fs};
use std::os::unix::fs::PermissionsExt;
const LIMIT:usize=20+1024*1024*3;
const HELPER:&str="tt-visual-preview";

#[derive(Debug)]
pub struct Frame {pub width:usize,pub height:usize,pub pages:u32,pixels:Vec<u8>}
impl Frame {
    pub fn rgb(&self)->&[u8]{&self.pixels}
    pub fn pixel(&self,x:usize,y:usize)->[u8;3] {
        let Some(max_x)=self.width.checked_sub(1) else {return [0;3];};
        let Some(max_y)=self.height.checked_sub(1) else {return [0;3];};
        let Some(i)=y.min(max_y).checked_mul(self.width)
            .and_then(|v|v.checked_add(x.min(max_x))).and_then(|v|v.checked_mul(3))
        else {return [0;3];};
        let Some(end)=i.checked_add(3) else {return [0;3];};
        self.pixels.get(i..end).and_then(|p|p.try_into().ok()).unwrap_or([0;3])
    }
}
fn executable(path:&str)->bool {fs::metadata(path).is_ok_and(|m|m.is_file()&&m.permissions().mode()&0o111!=0)}
pub fn installed()->bool {crate::helper_paths::find(HELPER).is_some()&&executable("/usr/bin/timeout")&&executable("/usr/bin/prlimit")}
pub fn format(path:&Path)->Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str(){
        "png"|"jpg"|"jpeg"|"webp"|"gif"|"bmp"=>Some("image"),
        "pdf"=>Some("pdf"),_=>None,
    }
}
fn parse(bytes:&[u8],w:u32,h:u32,page:u32)->io::Result<Frame> {
    let invalid=||io::Error::other("invalid visual preview response");
    if bytes.len()<20||&bytes[..8]!=b"TTVRGB01"{return Err(invalid());}
    let n=|i:usize|->io::Result<u32>{
        let raw=bytes.get(i..i+4).and_then(|p|p.try_into().ok()).ok_or_else(invalid)?;
        Ok(u32::from_le_bytes(raw))
    };
    let (width,height,pages)=(n(8)?,n(12)?,n(16)?);
    if width==0||height==0||width>1024||height>1024||width>w||height>h||page==0||pages<page||pages>10000||bytes.len()!=20+width as usize*height as usize*3 {return Err(invalid());}
    Ok(Frame{width:width as usize,height:height as usize,pages,pixels:bytes[20..].to_vec()})
}
pub fn render(path:&Path,page:u32,w:u32,h:u32)->io::Result<Frame>{
    render_version(path,page,w,h,"rgb-v1",160)
}
pub fn render_hd(path:&Path,page:u32,w:u32,h:u32)->io::Result<Frame>{
    render_version(path,page,w,h,"rgb-v2",1024)
}
fn render_version(path:&Path,page:u32,w:u32,h:u32,version:&str,max:u32)->io::Result<Frame>{
    if !(1..=max).contains(&w)||!(1..=max).contains(&h)||page==0||page>10000{return Err(io::Error::other("invalid preview bounds"));}
    if !installed(){return Err(io::Error::other("Visual Preview add-on not installed"));}
    let helper=crate::helper_paths::find(HELPER).ok_or_else(||io::Error::other("Visual Preview helper unavailable"))?;
    let kind=format(path).ok_or_else(||io::Error::other("unsupported visual format"))?;
    let limit=20+w as usize*h as usize*3;
    let session=ReadSession::open(path)?;
    if session.len()>16*1024*1024{return Err(io::Error::other("Visual Preview input exceeds 16 MiB"));}
    let mut child=Command::new("/usr/bin/timeout")
        .args(["--kill-after=1s","3s","/usr/bin/prlimit","--as=805306368","--cpu=3","--"]).arg(helper).args([version,kind])
        .args([page.to_string(),w.to_string(),h.to_string()])
        .env_clear().env("PATH","/usr/bin:/bin").env("LC_ALL","C").current_dir("/")
        .stdin(Stdio::from(session.file().try_clone()?)).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let mut bytes=Vec::new();
    let Some(stdout)=child.stdout.take() else {
        let _=child.kill();
        child.wait()?;
        return Err(io::Error::other("visual preview output pipe unavailable"));
    };
    let result=stdout.take((limit.min(LIMIT)+1) as u64).read_to_end(&mut bytes);
    let status=child.wait()?;
    result?;
    session.verify()?;
    if !status.success()||bytes.len()>limit.min(LIMIT){return Err(io::Error::other("Visual Preview failed or exceeded resource limits"));}
    parse(&bytes,w,h,page)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn invalid_frame_coordinates_and_headers_are_bounded(){
        let mut data=b"TTVRGB01".to_vec();
        for n in [u32::MAX,u32::MAX,1]{data.extend(n.to_le_bytes());}
        assert!(parse(&data,u32::MAX,u32::MAX,1).is_err());
        for len in 0..20 {assert!(parse(&data[..len],1024,1024,1).is_err());}
        let mut frame=Frame{width:1,height:1,pages:1,pixels:vec![1,2,3]};
        assert_eq!(frame.pixel(usize::MAX,usize::MAX),[1,2,3]);
        frame.width=0;
        assert_eq!(frame.pixel(0,0),[0;3]);
        frame.width=usize::MAX;frame.height=usize::MAX;
        assert_eq!(frame.pixel(usize::MAX,usize::MAX),[0;3]);
    }
    #[test]fn strict_pixel_protocol(){
        let mut data=b"TTVRGB01".to_vec();
        for n in [2u32,1,3]{data.extend(n.to_le_bytes());}
        data.extend([255,0,0,0,255,0]);
        let f=parse(&data,2,2,1).unwrap();
        assert_eq!(f.pixel(1,0),[0,255,0]);
        assert_eq!(f.pages,3);
        assert!(parse(&data,1,2,1).is_err());
        assert!(parse(&data,2,2,4).is_err());
        data.push(27);assert!(parse(&data,2,2,1).is_err());
    }
}
