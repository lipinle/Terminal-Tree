//! Bounded Sixel encoder for the dedicated expanded preview screen.
//! Fixed 216-color palette; no escape sequence from an input file is copied.
use std::io::{self,Write};
use terminal_tree::visual_preview::Frame;

fn quantize(v:u8)->usize{(usize::from(v)*5+127)/255}
fn run(out:&mut impl Write,value:u8,n:usize)->io::Result<()>{
    if n>=4{write!(out,"!{n}{}",char::from(value))}
    else{for _ in 0..n{out.write_all(&[value])?;}Ok(())}
}
fn encode(out:&mut impl Write,w:usize,h:usize,rgb:&[u8])->io::Result<()>{
    if w==0||h==0||w>1024||h>1024||rgb.len()!=w*h*3{return Err(io::Error::other("invalid sixel frame"));}
    let palette:Vec<usize>=rgb.chunks_exact(3).map(|p|quantize(p[0])*36+quantize(p[1])*6+quantize(p[2])).collect();
    write!(out,"\x1bP0;1;0q\"1;1;{w};{h}")?;
    let mut used=[false;216];for &i in &palette{used[i]=true;}
    for (i,used) in used.iter().enumerate(){if *used{write!(out,"#{i};2;{};{};{}",i/36*20,i/6%6*20,i%6*20)?;}}
    let mut masks=vec![0u8;216*w];
    for y in (0..h).step_by(6){
        masks.fill(0);
        let mut last=[0usize;216];
        for dy in 0..6.min(h-y){
            for x in 0..w{let c=palette[(y+dy)*w+x];masks[c*w+x]|=1<<dy;last[c]=last[c].max(x+1);}
        }
        let mut first=true;
        for (c,end) in last.into_iter().enumerate(){
            if end==0{continue;}
            if !first{out.write_all(b"$")?;}first=false;
            write!(out,"#{c}")?;
            let row=&masks[c*w..c*w+end];
            let mut x=0;
            while x<row.len(){let value=row[x];let mut n=1;while x+n<row.len()&&row[x+n]==value{n+=1;}run(out,value+63,n)?;x+=n;}
        }
        if y+6<h{out.write_all(b"-")?;}
    }
    out.write_all(b"\x1b\\")
}
pub fn show(frame:&Frame,x:u16,y:u16)->io::Result<()>{
    use crossterm::{cursor::MoveTo,queue};
    let mut buf=Vec::new();queue!(buf,MoveTo(x,y))?;
    encode(&mut buf,frame.width,frame.height,frame.rgb())?;
    let mut out=io::stdout().lock();out.write_all(&buf)?;out.flush()
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn encodes_two_colors_and_bands(){
        let mut out=Vec::new();
        let mut rgb=vec![0;2*7*3];rgb[3]=255;
        encode(&mut out,2,7,&rgb).unwrap();
        let s=std::str::from_utf8(&out).unwrap();
        assert!(s.starts_with("\x1bP0;1;0q\"1;1;2;7"));
        assert!(s.contains("#180;2;100;0;0"));
        assert!(s.contains('-'));assert!(s.ends_with("\x1b\\"));
        assert!(encode(&mut out,1025,1,&[]).is_err());
    }
}
