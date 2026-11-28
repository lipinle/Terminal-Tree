#![forbid(unsafe_code)]
use std::io::{self,Read,Write,Cursor};
use image::{DynamicImage,ImageReader,Limits};
use hayro::{hayro_syntax::Pdf,hayro_interpret::InterpreterSettings,RenderCache,RenderSettings,PixmapSettings};
const MAX_INPUT:usize=16*1024*1024;

fn decode_image(data:&[u8])->Result<DynamicImage,Box<dyn std::error::Error>> {
    let mut reader=ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    let mut limits=Limits::default();
    limits.max_image_width=Some(8192);
    limits.max_image_height=Some(8192);
    limits.max_alloc=Some(96*1024*1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}
fn pdf(data:Vec<u8>,page:usize,width:u32,height:u32)->Result<(DynamicImage,u32),Box<dyn std::error::Error>> {
    let pdf=Pdf::new(data).map_err(|_|"invalid or unsupported PDF")?;
    let pages=pdf.pages();
    if pages.is_empty()||pages.len()>10000 {return Err("invalid page count".into());}
    let p=pages.get(page.checked_sub(1).ok_or("invalid page")?).ok_or("page out of range")?;
    let (w,h)=p.render_dimensions();
    if !w.is_finite()||!h.is_finite()||w<=0.0||h<=0.0 {return Err("invalid page dimensions".into());}
    let scale=(width as f32/w).min(height as f32/h).min(2.0);
    if w*scale<1.0||h*scale<1.0 {return Err("invalid scaled page size".into());}
    let cache=RenderCache::new();
    let pixmap=hayro::render(p,&cache,&InterpreterSettings::default(),&RenderSettings::default(),&PixmapSettings{x_scale:scale,y_scale:scale,..Default::default()});
    let png=pixmap.into_png().map_err(|_|"PDF raster encoding failed")?;
    Ok((decode_image(&png)?,pages.len() as u32))
}
fn run()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args_os().skip(1).map(|arg|arg.into_string().map_err(|_|"invalid UTF-8 argument")).collect::<std::result::Result<Vec<_>,_>>()?;
    if args.len()!=5||!matches!(args[0].as_str(),"rgb-v1"|"rgb-v2") {return Err("usage: tt-visual-preview rgb-v1|rgb-v2 image|pdf PAGE WIDTH HEIGHT < file".into());}
    let max=if args[0]=="rgb-v2"{1024}else{160};
    let page:usize=args[2].parse()?;
    let w:u32=args[3].parse()?;
    let h:u32=args[4].parse()?;
    if !(1..=max).contains(&w)||!(1..=max).contains(&h)||page==0 {return Err("invalid preview bounds".into());}
    let mut data=Vec::new();
    io::stdin().take((MAX_INPUT+1) as u64).read_to_end(&mut data)?;
    if data.len()>MAX_INPUT {return Err("input exceeds 16 MiB".into());}
    let (image,pages)=match args[1].as_str() {
        "image" if page==1=>(decode_image(&data)?,1),
        "pdf"=>pdf(data,page,w,h)?,
        _=>return Err("unsupported input type".into()),
    };
    let image=if args[0]=="rgb-v2"{image.resize(w,h,image::imageops::FilterType::Lanczos3)}else{image.thumbnail(w,h)}.to_rgba8();
    let mut output=io::stdout().lock();
    output.write_all(b"TTVRGB01")?;
    for n in [image.width(),image.height(),pages] {output.write_all(&n.to_le_bytes())?;}
    for pixel in image.pixels() {
        let alpha=u16::from(pixel[3]);
        let rgb=[0,1,2].map(|i|((u16::from(pixel[i])*alpha+255*(255-alpha))/255) as u8);
        output.write_all(&rgb)?;
    }
    output.flush()?;
    Ok(())
}
fn main(){if let Err(error)=run(){eprintln!("preview: {error}");std::process::exit(1);}}
