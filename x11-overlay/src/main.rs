#![forbid(unsafe_code)]
//! Experimental click-through X11 child window. No screen capture, input grab,
//! foreign window mutation, or top-level always-on-top window.
use std::{error::Error,io::{self,Read,Write},sync::mpsc,thread,time::Duration};
use x11rb::{connection::Connection,protocol::{xproto::{self,ConnectionExt,AtomEnum,Window,ImageFormat,WindowClass,CreateWindowAux,CreateGCAux,MapState,VisualClass,ImageOrder,ClipOrdering},shape::{self,ConnectionExt as _,SO,SK}},rust_connection::RustConnection,COPY_DEPTH_FROM_PARENT,COPY_FROM_PARENT};
type Result<T>=std::result::Result<T,Box<dyn Error>>;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Rect{x:u16,y:u16,w:u16,h:u16}
fn parse_window(s:&str)->Result<Window>{
    let id=if let Some(hex)=s.strip_prefix("0x"){u32::from_str_radix(hex,16)?}else{s.parse()?};
    if id==0{return Err("invalid window ID".into());}Ok(id)
}
fn atom(c:&RustConnection,name:&[u8])->Result<u32>{Ok(c.intern_atom(false,name)?.reply()?.atom)}
fn active(c:&RustConnection,root:Window,a:u32)->Result<Option<Window>>{
    let p=c.get_property(false,root,a,AtomEnum::WINDOW,0,1)?.reply()?;
    Ok(p.value32().and_then(|mut v|v.next()))
}
fn is_terminal(c:&RustConnection,parent:Window)->Result<bool>{
    let p=c.get_property(false,parent,AtomEnum::WM_CLASS,AtomEnum::STRING,0,128)?.reply()?;
    Ok(p.value.split(|b|*b==0).any(|s|s.eq_ignore_ascii_case(b"gnome-terminal")||s.eq_ignore_ascii_case(b"gnome-terminal-server")||s.eq_ignore_ascii_case(b"org.gnome.Terminal")))
}
// WM_NORMAL_HINTS uses ICCCM's 32-bit size-hint layout. Without resize
// increments we cannot infer the cell grid, and refuse automatic placement.
fn grid(w:u16,h:u16,cols:u16,rows:u16,hints:&[u32])->Result<(u16,u16,u16,u16)>{
    if hints.len()<11||hints[0]&(1<<6)==0||cols==0||rows==0{return Err("terminal resize increments unavailable".into());}
    let (cw,ch)=(hints[9],hints[10]);
    if !(4..=64).contains(&cw)||!(8..=128).contains(&ch){return Err("invalid terminal cell size".into());}
    let ew=u32::from(w).checked_sub(u32::from(cols)*cw).ok_or("grid wider than client")?;
    let eh=u32::from(h).checked_sub(u32::from(rows)*ch).ok_or("grid taller than client")?;
    if ew>128||eh>256{return Err("ambiguous terminal padding".into());}
    // We don't know how padding is divided across the sides. Restrict the
    // image to the intersection of all possible positions of the requested
    // cell rectangle. This trades a little area for safer placement.
    Ok((ew as u16,eh as u16,cw as u16,ch as u16))
}
fn probe(cols:u16,rows:u16,pinned:Option<Window>)->Result<()>{
    let (c,s)=x11rb::connect(None)?;
    let root=c.setup().roots.get(s).ok_or("X11 screen not found")?.root;
    let parent=active(&c,root,atom(&c,b"_NET_ACTIVE_WINDOW")?)?.ok_or("no active window")?;
    if pinned.is_some_and(|id|id!=parent)||parent==root||!is_terminal(&c,parent)?{return Err("active window is not the selected GNOME Terminal client".into());}
    let g=c.get_geometry(parent)?.reply()?;
    if g.root!=root||!matches!(g.depth,24|32)||c.get_window_attributes(parent)?.reply()?.map_state!=MapState::VIEWABLE{return Err("unsupported terminal client".into());}
    let prop=c.get_property(false,parent,AtomEnum::WM_NORMAL_HINTS,AtomEnum::WM_SIZE_HINTS,0,18)?.reply()?;
    let hints:Vec<_>=prop.value32().ok_or("invalid size hints")?.collect();
    let (ew,eh,cw,ch)=grid(g.width,g.height,cols,rows,&hints)?;
    println!("{parent} {ew} {eh} {cw} {ch} {} {}",g.width,g.height);
    Ok(())
}
fn fits(rect:Rect,w:u16,h:u16)->bool{
    rect.w>0&&rect.h>0&&u32::from(rect.x)+u32::from(rect.w)<=u32::from(w)&&u32::from(rect.y)+u32::from(rect.h)<=u32::from(h)&&rect.x<=i16::MAX as u16&&rect.y<=i16::MAX as u16
}
fn make(c:&RustConnection,root:Window,parent:Window,rect:Rect,w:u16,h:u16,rgb:&[u8])->Result<Window>{
    let g=c.get_geometry(parent)?.reply()?;
    let a=c.get_window_attributes(parent)?.reply()?;
    if g.root!=root||!matches!(g.depth,24|32)||!fits(rect,g.width,g.height)||a.map_state!=MapState::VIEWABLE{return Err("unsupported or changed target geometry".into());}
    let screen=c.setup().roots.iter().find(|s|s.root==root).ok_or("screen not found")?;
    let visual=screen.allowed_depths.iter().flat_map(|d|&d.visuals).find(|v|v.visual_id==a.visual).ok_or("visual not found")?;
    if visual.class!=VisualClass::TRUE_COLOR||visual.red_mask!=0xff0000||visual.green_mask!=0x00ff00||visual.blue_mask!=0x0000ff{return Err("unsupported X11 visual".into());}
    if !c.setup().pixmap_formats.iter().any(|f|f.depth==g.depth&&f.bits_per_pixel==32&&f.scanline_pad==32){return Err("unsupported X11 pixel format".into());}
    if w==0||h==0||w>1024||h>1024||rect.w!=w||rect.h!=h||rgb.len()!=usize::from(w)*usize::from(h)*3{return Err("invalid pixel frame".into());}
    let version=c.shape_query_version()?.reply()?;
    if (version.major_version,version.minor_version)<(1,1){return Err("X Shape input transparency unavailable".into());}
    let win=c.generate_id()?;
    let pix=c.generate_id()?;
    let gc=c.generate_id()?;
    c.create_pixmap(g.depth,pix,parent,w,h)?.check()?;
    c.create_gc(gc,pix,&CreateGCAux::new())?.check()?;
    let stride=usize::from(w)*4;
    let rows_per=(60*1024/stride).max(1);
    let little=c.setup().image_byte_order==ImageOrder::LSB_FIRST;
    for y in (0..usize::from(h)).step_by(rows_per){
        let rows=rows_per.min(usize::from(h)-y);
        let mut data=Vec::with_capacity(rows*stride);
        for p in rgb[y*usize::from(w)*3..(y+rows)*usize::from(w)*3].chunks_exact(3){
            let alpha=if g.depth==32{255}else{0};
            if little{data.extend([p[2],p[1],p[0],alpha]);}else{data.extend([alpha,p[0],p[1],p[2]]);}
        }
        c.put_image(ImageFormat::Z_PIXMAP,pix,gc,w,rows as u16,0,y as i16,0,g.depth,&data)?.check()?;
    }
    c.create_window(COPY_DEPTH_FROM_PARENT,win,parent,rect.x as i16,rect.y as i16,w,h,0,WindowClass::INPUT_OUTPUT,COPY_FROM_PARENT,&CreateWindowAux::new().background_pixmap(pix))?.check()?;
    // Empty input region: mouse events go through to the terminal.
    c.shape_rectangles(SO::SET,SK::INPUT,ClipOrdering::UNSORTED,win,0,0,&[])?.check()?;
    c.free_gc(gc)?.check()?;
    c.free_pixmap(pix)?.check()?; // background retains its reference
    c.map_window(win)?.check()?;
    c.flush()?;
    Ok(win)
}
fn run()->Result<()>{
    let a:Vec<_>=std::env::args_os().skip(1).map(|arg|arg.into_string().map_err(|_|"invalid UTF-8 argument")).collect::<std::result::Result<Vec<_>,_>>()?;
    if a.first().map(String::as_str)==Some("--probe"){
        if !matches!(a.len(),3|4){return Err("usage: tt-x11-overlay --probe COLS ROWS [WINDOW_ID]".into());}
        return probe(a[1].parse()?,a[2].parse()?,a.get(3).map(|s|parse_window(s)).transpose()?);
    }
    if !matches!(a.len(),5|7){return Err("usage: tt-x11-overlay WINDOW_ID X Y WIDTH HEIGHT [CLIENT_W CLIENT_H] < RGB stream".into());}
    let parent=parse_window(&a[0])?;
    let rect=Rect{x:a[1].parse()?,y:a[2].parse()?,w:a[3].parse()?,h:a[4].parse()?};
    if !(1..=1024).contains(&rect.w)||!(1..=1024).contains(&rect.h){return Err("invalid dimensions".into());}
    let mut rgb=vec![0;usize::from(rect.w)*usize::from(rect.h)*3];
    io::stdin().read_exact(&mut rgb)?;
    let (c,s)=x11rb::connect(None)?;
    let root=c.setup().roots.get(s).ok_or("X11 screen not found")?.root;
    let active_atom=atom(&c,b"_NET_ACTIVE_WINDOW")?;
    if parent==root||!is_terminal(&c,parent)?||active(&c,root,active_atom)?!=Some(parent){return Err("target must be the active GNOME Terminal client".into());}
    let g=c.get_geometry(parent)?.reply()?;
    let initial=(g.width,g.height);
    if a.len()==7&&initial!=(a[5].parse()?,a[6].parse()?){return Err("client resized after calibration".into());}
    let win=make(&c,root,parent,rect,rect.w,rect.h,&rgb)?;
    io::stdout().write_all(b"OK\n")?;io::stdout().flush()?;
    // A socket owned by the core remains open while this image is needed.
    let (tx,rx)=mpsc::channel();
    thread::spawn(move||{let mut byte=[0];let _=io::stdin().read(&mut byte);let _=tx.send(());});
    loop{
        if rx.recv_timeout(Duration::from_millis(100)).is_ok(){break;}
        let g=match c.get_geometry(parent)?.reply(){Ok(g)=>g,Err(_)=>break};
        if (g.width,g.height)!=initial||active(&c,root,active_atom)?!=Some(parent)||c.get_window_attributes(parent)?.reply()?.map_state!=MapState::VIEWABLE{break;}
    }
    let _=c.destroy_window(win)?.check();
    c.flush()?;
    Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("x11 overlay: {e}");std::process::exit(1);}}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn grid_inference(){
        let mut hints=[0;18];hints[0]=1<<6;hints[9]=10;hints[10]=20;
        assert_eq!(grid(804,530,80,25,&hints).unwrap(),(4,30,10,20));
        assert!(grid(799,530,80,25,&hints).is_err());
        assert!(grid(1000,530,80,25,&hints).is_err());
        hints[0]=0;assert!(grid(804,530,80,25,&hints).is_err());
    }
    #[test]fn bounds(){assert!(fits(Rect{x:10,y:10,w:20,h:20},30,30));assert!(!fits(Rect{x:65535,y:0,w:10,h:10},65535,50));}
    #[test]#[ignore]fn xvfb_click_through(){
        let (c,s)=x11rb::connect(None).unwrap();let root=c.setup().roots[s].root;
        let parent=c.generate_id().unwrap();
        c.create_window(COPY_DEPTH_FROM_PARENT,parent,root,0,0,200,200,0,WindowClass::INPUT_OUTPUT,COPY_FROM_PARENT,&CreateWindowAux::new()).unwrap().check().unwrap();
        c.map_window(parent).unwrap().check().unwrap();
        let child=make(&c,root,parent,Rect{x:4,y:5,w:20,h:20},20,20,&vec![255;1200]).unwrap();
        let g=c.get_geometry(child).unwrap().reply().unwrap();assert_eq!((g.x,g.y,g.width,g.height),(4,5,20,20));
        assert!(c.shape_get_rectangles(child,SK::INPUT).unwrap().reply().unwrap().rectangles.is_empty());
        c.destroy_window(parent).unwrap().check().unwrap();
    }
}
