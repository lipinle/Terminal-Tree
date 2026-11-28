//! Larger image/PDF view for terminals without native bitmap graphics.
use crossterm::{cursor::MoveTo,event::{self,Event,KeyCode,KeyEventKind,MouseEventKind},queue,style::{Color,Print,ResetColor,SetForegroundColor,SetBackgroundColor},terminal::{self,Clear,ClearType}};
use std::{io::{self,Write},path::Path,process::{Command,Stdio}};
use terminal_tree::{display_name,visual_preview};

pub fn show(path:&Path)->io::Result<()>{
    let mut page=1u32;
    loop{
        let (w,h)=terminal::size()?;
        if w<8||h<6{return Err(io::Error::other("preview needs a larger terminal"));}
        let width=u32::from(w-1).min(160);
        let height=u32::from(h-4).saturating_mul(2).min(160);
        let frame=visual_preview::render(path,page,width,height)?;
        let mut out=Vec::new();
        queue!(out,MoveTo(0,0),ResetColor,Clear(ClearType::All),SetForegroundColor(Color::Cyan),Print(super::clip(&format!("Expanded Preview | {}",display_name(path.as_os_str())),usize::from(w-1))),ResetColor)?;
        queue!(out,MoveTo(0,1),Print(super::clip(&format!("Page {page}/{} | {}x{} terminal pixels",frame.pages,frame.width,frame.height),usize::from(w-1))))?;
        let left=(usize::from(w-1).saturating_sub(frame.width)/2) as u16;
        for y in (0..frame.height).step_by(2){
            queue!(out,MoveTo(left,2+(y/2) as u16))?;
            for x in 0..frame.width{
                let [r,g,b]=frame.pixel(x,y);let [r2,g2,b2]=frame.pixel(x,y+1);
                queue!(out,SetForegroundColor(Color::Rgb{r,g,b}),SetBackgroundColor(Color::Rgb{r:r2,g:g2,b:b2}),Print("▀"))?;
            }
        }
        queue!(out,ResetColor,MoveTo(0,h-1),Print(super::clip("Esc back | Left/Right page | O open original in desktop viewer",usize::from(w-1))))?;
        let mut stdout=io::stdout().lock();stdout.write_all(&out)?;stdout.flush()?;drop(stdout);
        loop{
            match event::read()?{
                Event::Resize(_,_)=>break,
                Event::Key(k) if matches!(k.kind,KeyEventKind::Press|KeyEventKind::Repeat)=>match k.code{
                    KeyCode::Esc|KeyCode::Char('q')=>return Ok(()),
                    KeyCode::Left|KeyCode::PageUp if page>1=>{page-=1;break;},
                    KeyCode::Right|KeyCode::PageDown if page<frame.pages=>{page+=1;break;},
                    KeyCode::Char('o'|'O')=>{
                        if super::choose("Open this file in your configured desktop application?",&["Cancel".into(),display_name(path.as_os_str())])?==Some(1){
                            // Explicit action. The OS association is trusted by the user.
                            let metadata=std::fs::symlink_metadata(path)?;
                            if !metadata.is_file(){return Err(io::Error::other("original file is no longer regular"));}
                            let status=Command::new("/usr/bin/timeout").args(["--kill-after=1s","3s","/usr/bin/xdg-open"]).arg(path)
                                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
                            if !status.success(){super::page("Desktop viewer",&["The opener did not report success. Check whether the viewer opened.".into()])?;}
                            return Ok(());
                        }
                        break;
                    }
                    _=>(),
                },
                Event::Mouse(m)=>match m.kind{
                    MouseEventKind::ScrollUp if page>1=>{page-=1;break;},
                    MouseEventKind::ScrollDown if page<frame.pages=>{page+=1;break;},
                    _=>(),
                },
                _=>(),
            }
        }
    }
}
