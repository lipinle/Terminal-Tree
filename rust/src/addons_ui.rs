//! Native companion settings; no installation or execution on selection.
use crossterm::{cursor::MoveTo,event::{self,Event,KeyCode,KeyEventKind,MouseButton,MouseEventKind},queue,style::{Color,Print,ResetColor,SetForegroundColor,SetBackgroundColor},terminal::{self,Clear,ClearType}};
use std::io::{self,Write};
use terminal_tree::external_tools::Tool;
const NAMES:[&str;4]=["eza","7-Zip","Git","Visual Preview"];
const DESCRIPTIONS:[&str;4]=["Detailed directory listing","Inspect archive contents","Repository status and local Git manager","Render image thumbnails and PDF pages"];
const DETAILS:[&str;4]=["Run manually from Tools. Read-only listing.","Run manually from Tools. Read-only archive inspection.","Explicit trust required. Local editing is separate from the file journal.","Runs on selection when enabled. Separate Rust helper."];
pub fn show(prefs:&mut super::app_settings::Preferences,last:Option<&str>)->io::Result<()>{
    let mut selected=0usize;
    loop{
        let (w,h)=terminal::size()?;
        let width=usize::from(w.saturating_sub(1));
        let installed=[Tool::Eza.installed(),Tool::SevenZip.installed(),Tool::Git.installed(),terminal_tree::visual_preview::installed()];
        let enabled=[prefs.companions[0],prefs.companions[1],prefs.companions[2],prefs.visual_preview];
        let mut out=Vec::new();
        queue!(out,MoveTo(0,0),ResetColor,Clear(ClearType::All),SetForegroundColor(prefs.accent),Print(super::clip("Add-ons / Integrations",width)),ResetColor)?;
        for i in 0..4{
            if i+1>=usize::from(h.saturating_sub(1)){break;}
            let state=if installed[i]{"Installed"}else{"Not installed"};
            let on=if enabled[i]{"Enabled"}else{"Disabled"};
            queue!(out,MoveTo(0,(i+1) as u16))?;
            if i==selected{queue!(out,SetBackgroundColor(prefs.selection))?;}
            let name=format!("{} | {} | {}",NAMES[i],state,on);
            queue!(out,Print(super::clip(&name,width)),ResetColor)?;
            if width>=82{
                queue!(out,MoveTo(47,(i+1) as u16),SetForegroundColor(if installed[i]&&enabled[i]{Color::Green}else{Color::DarkGrey}),Print(super::clip(DESCRIPTIONS[i],width-47)),ResetColor)?;
            }
        }
        if h>7{
            queue!(out,MoveTo(0,5),SetForegroundColor(prefs.accent),Print(super::clip(&format!("Selected: {} — {}",NAMES[selected.min(3)],DESCRIPTIONS[selected.min(3)]),width)),ResetColor)?;
            queue!(out,MoveTo(0,6),Print(super::clip(if selected==4{"Installation help (Enter opens)"}else{DETAILS[selected.min(3)]},width)))?;
        }
        if h>9{
            queue!(out,MoveTo(0,8),Print(super::clip(&format!("Last run: {}",last.unwrap_or("None")),width)))?;
        }
        if h>11{
            queue!(out,MoveTo(0,10),SetForegroundColor(prefs.accent),Print(super::clip("Enter / click: toggle   H: installation help   Esc: back",width)),ResetColor)?;
        }
        if h>1{queue!(out,MoveTo(0,h-1),Print(super::clip("Up/Down select | Enter toggle | H help | Esc back",width)))?;}
        let mut stdout=io::stdout().lock();stdout.write_all(&out)?;stdout.flush()?;drop(stdout);
        let action=match event::read()?{
            Event::Key(k) if matches!(k.kind,KeyEventKind::Press|KeyEventKind::Repeat)=>match k.code{
                KeyCode::Esc|KeyCode::Char('q')=>return Ok(()),
                KeyCode::Up=>{selected=selected.saturating_sub(1);None},
                KeyCode::Down=>{selected=(selected+1).min(3);None},
                KeyCode::Enter=>Some(selected),
                KeyCode::Char('h'|'H')=>Some(4),
                _=>None,
            },
            Event::Mouse(m) if m.kind==MouseEventKind::Down(MouseButton::Left)&&m.row>=1&&m.row<=4=>{selected=usize::from(m.row-1);Some(selected)},
            Event::Mouse(m) if m.kind==MouseEventKind::Down(MouseButton::Left)&&m.row==10=>Some(4),
            _=>None,
        };
        match action{
            Some(i @ 0..=2)=>prefs.companions[i]=!prefs.companions[i],
            Some(3)=>prefs.visual_preview=!prefs.visual_preview,
            Some(4)=>super::page("Add-on installation help",&[
                "eza / 7-Zip / Git: distribution packages.".into(),
                "sudo apt install eza 7zip git".into(),
                "Visual Preview: build separately from the source checkout.".into(),
                "cargo +stable build --release --manifest-path visual-preview/Cargo.toml".into(),
                "sudo install -m 755 visual-preview/target/release/tt-visual-preview /usr/local/bin/tt-visual-preview".into(),
                "Enable Visual Preview and select an image/PDF.".into(),
                "No add-on is installed or updated automatically.".into(),
            ])?,
            _=>(),
        }
    }
}
