//! Explicit user-command console. Not a sandbox and not a PTY emulator.
//! Never execute file names, previews, imported settings, or history automatically.
use crossterm::{
    cursor::MoveTo,
    event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    queue,
    style::{Print, SetBackgroundColor, SetForegroundColor},
};
use std::{io::{self,Read,Write},path::Path,process::{Child,Command,Stdio},time::{Duration,Instant}};
use std::os::unix::{ffi::OsStrExt,process::CommandExt};
use rustix::{fs::{fcntl_getfl,fcntl_setfl,OFlags},process::{kill_process_group,Pid,Signal}};

const OUTPUT_LIMIT:usize=32*1024;
const HISTORY_LIMIT:usize=32;
const SCROLLBACK_LIMIT:usize=128*1024;
const SCROLLBACK_LINES:usize=2048;

struct Job {
    child:Child,
    bytes:Vec<u8>,
    started:Instant,
    directory:std::path::PathBuf,
    command:String,
    truncated:bool,
    stopped:Option<&'static str>,
}

fn nonblocking(fd:impl std::os::fd::AsFd)->io::Result<()> {
    let flags=fcntl_getfl(&fd)?;
    fcntl_setfl(&fd,flags|OFlags::NONBLOCK)?;
    Ok(())
}
fn drain(pipe:&mut impl Read, bytes:&mut Vec<u8>, truncated:&mut bool)->io::Result<()> {
    let mut buffer=[0;4096];
    // Budget per poll, including discarded output.
    for _ in 0..8 {
        match pipe.read(&mut buffer) {
            Ok(0)=>break,
            Ok(n)=>{
                let keep=n.min(OUTPUT_LIMIT.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..keep]);
                *truncated|=keep!=n;
            }
            Err(e) if e.kind()==io::ErrorKind::WouldBlock=>break,
            Err(e) if e.kind()==io::ErrorKind::Interrupted=>continue,
            Err(e)=>return Err(e),
        }
    }
    Ok(())
}

impl Job {
    fn start(command:&str, directory:&Path)->io::Result<Self> {
        let mut child=Command::new("/bin/bash")
            .args(["--noprofile","--norc","-c",command])
            .env_remove("BASH_ENV").env_remove("ENV")
            .env("TERM","dumb")
            .current_dir(directory)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .process_group(0).spawn()?;
        let result=nonblocking(child.stdout.as_ref().expect("piped stdout"))
            .and_then(|_|nonblocking(child.stderr.as_ref().expect("piped stderr")));
        if let Err(error)=result {
            if let Some(pid)=Pid::from_raw(child.id() as i32) { let _=kill_process_group(pid,Signal::Kill); }
            let _=child.kill(); let _=child.wait(); return Err(error);
        }
        Ok(Self{child,bytes:Vec::new(),started:Instant::now(),directory:directory.to_path_buf(),command:command.to_owned(),truncated:false,stopped:None})
    }
    fn stop(&mut self, reason:&'static str) {
        if self.stopped.is_some(){return;}
        self.stopped=Some(reason);
        // The unreaped child owns the group ID. Never signal after reaping.
        if let Some(pid)=Pid::from_raw(self.child.id() as i32) {let _=kill_process_group(pid,Signal::Kill);}
        let _=self.child.kill();
    }
    fn collect(&mut self)->io::Result<()> {
        if let Some(pipe)=&mut self.child.stdout {drain(pipe,&mut self.bytes,&mut self.truncated)?;}
        if let Some(pipe)=&mut self.child.stderr {drain(pipe,&mut self.bytes,&mut self.truncated)?;}
        Ok(())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(),Ok(None)) {
            self.stop("closed");
            let _=self.child.wait();
        }
    }
}

#[derive(Default)]
pub struct Panel {
    pub focused:bool,
    input:String,
    cursor:usize,
    history:Vec<String>,
    history_at:Option<usize>,
    job:Option<Job>,
    shell:Option<super::pty_shell::Shell>,
    output:Vec<String>,
    scroll:usize,
}
fn visible(bytes:&[u8])->String {
    terminal_tree::display_name(std::ffi::OsStr::from_bytes(bytes))
}
impl Panel {
    pub fn running(&self)->bool {self.job.is_some()||self.shell.is_some()}
    pub fn interactive(&self)->bool{self.shell.is_some()}
    pub fn start_shell(&mut self,directory:&Path,cols:u16,rows:u16)->io::Result<()> {
        if self.running(){return Err(io::Error::other("stop the current command or shell first"));}
        self.shell=Some(super::pty_shell::Shell::start(directory,cols,rows)?);self.scroll=0;self.focused=true;Ok(())
    }
    pub fn close_shell(&mut self){self.shell=None;self.focused=false;self.scroll=0;}
    pub fn resize_shell(&mut self,cols:u16,rows:u16)->io::Result<()>{if let Some(s)=&mut self.shell{s.resize(cols,rows)?;}Ok(())}
    fn previous_boundary(&self)->usize {
        self.input[..self.cursor].char_indices().next_back().map_or(0,|(i,_)|i)
    }
    fn next_boundary(&self)->usize {
        self.input[self.cursor..].chars().next().map_or(self.cursor,|c|self.cursor+c.len_utf8())
    }
    fn trim_scrollback(&mut self) {
        let mut bytes=0usize;
        let keep=self.output.iter().rev().take(SCROLLBACK_LINES).take_while(|line|{
            bytes=bytes.saturating_add(line.len());
            bytes<=SCROLLBACK_LIMIT
        }).count();
        self.output.drain(..self.output.len()-keep);
    }
    fn replace_input(&mut self,text:String){self.input=text;self.cursor=self.input.len();}
    fn clear_input(&mut self){self.input.clear();self.cursor=0;}
    fn prompt_text(&self,width:usize)->String{
        use unicode_width::UnicodeWidthStr;
        let mut left=visible(self.input[..self.cursor].as_bytes());
        let right=visible(self.input[self.cursor..].as_bytes());
        let room=width.saturating_sub(3);
        while left.width()>room.saturating_sub(1) && !left.is_empty() {
            let n=left.chars().next().map_or(0,char::len_utf8);
            left.drain(..n);
        }
        let tail=super::clip(&right,room.saturating_sub(left.width()));
        format!("$ {left}│{tail}")
    }
    pub fn tick(&mut self)->io::Result<bool> {
        if let Some(shell)=&mut self.shell {
            match shell.tick(){
                Ok(false)=>return Ok(false),
                Ok(true)=>self.output.extend(shell.lines()),
                Err(e)=>self.output.push(format!("[PTY error: {}]",visible(e.to_string().as_bytes()))),
            }
            self.close_shell();self.output.push("[Interactive shell closed]".into());self.trim_scrollback();
            return Ok(true);
        }
        let Some(job)=&mut self.job else{return Ok(false);};
        if job.started.elapsed()>=Duration::from_secs(60) {job.stop("60-second limit");}
        job.collect()?;
        let Some(status)=job.child.try_wait()? else{return Ok(false);};
        // Drain already queued data, without blocking on a detached writer.
        job.collect()?;
        let job=self.job.take().expect("running job");
        self.output.push(format!("$ {}",visible(job.command.as_bytes())));
        self.output.extend(job.bytes.split(|&b|b==b'\n').filter(|line|!line.is_empty()).map(visible));
        if job.truncated {self.output.push("[Output truncated at 32 KiB]".into());}
        self.output.push(match job.stopped {Some(reason)=>format!("[Stopped: {reason}]"),None=>format!("[{status}]")});
        self.trim_scrollback();self.scroll=0;
        Ok(true)
    }
    pub fn input(&mut self,key:KeyEvent,directory:&Path)->io::Result<()> {
        if !matches!(key.kind,KeyEventKind::Press|KeyEventKind::Repeat) {return Ok(());}
        if let Some(shell)=&mut self.shell{return shell.key(key);}
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c')=>{
                    if let Some(job)=&mut self.job {job.stop("cancelled");}
                    else {self.clear_input();self.history_at=None;}
                }
                KeyCode::Char('l')=>{self.output.clear();self.scroll=0;}
                KeyCode::Char('u') if !self.running()=>{self.input.drain(..self.cursor);self.cursor=0;},
                KeyCode::Char('a') if !self.running()=>self.cursor=0,
                KeyCode::Char('e') if !self.running()=>self.cursor=self.input.len(),
                _=>(),
            }
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::ALT){return Ok(());}
        match key.code {
            KeyCode::Esc=>self.focused=false,
            KeyCode::PageUp=>self.scroll=(self.scroll+3).min(self.output.len().saturating_sub(1)),
            KeyCode::PageDown=>self.scroll=self.scroll.saturating_sub(3),
            _ if self.running()=>(),
            KeyCode::Left=>self.cursor=self.previous_boundary(),
            KeyCode::Right=>self.cursor=self.next_boundary(),
            KeyCode::Home=>self.cursor=0,
            KeyCode::End=>self.cursor=self.input.len(),
            KeyCode::Backspace=>{let start=self.previous_boundary();self.input.drain(start..self.cursor);self.cursor=start;}
            KeyCode::Delete=>{let end=self.next_boundary();self.input.drain(self.cursor..end);}
            KeyCode::Up=>{
                if !self.history.is_empty() {
                    let index=self.history_at.map_or(self.history.len()-1,|n|n.saturating_sub(1));
                    self.history_at=Some(index);self.replace_input(self.history[index].clone());
                }
            }
            KeyCode::Down=>{
                if let Some(index)=self.history_at {
                    if index+1<self.history.len(){self.history_at=Some(index+1);self.replace_input(self.history[index+1].clone());}
                    else{self.history_at=None;self.clear_input();}
                }
            }
            KeyCode::Enter if key.kind==KeyEventKind::Press && !self.input.trim().is_empty()=>{
                let command=self.input.clone();
                let job=Job::start(&command,directory)?;
                if self.history.last()!=Some(&command) {
                    if self.history.len()==HISTORY_LIMIT {self.history.remove(0);}
                    self.history.push(command);
                }
                self.clear_input();self.history_at=None;self.scroll=0;
                self.job=Some(job);
            }
            KeyCode::Char(c) if !c.is_control()&&self.input.len()+c.len_utf8()<=4096=>{self.input.insert(self.cursor,c);self.cursor+=c.len_utf8();},
            _=>(),
        }
        Ok(())
    }
    pub fn scroll(&mut self,up:bool){
        let len=self.shell.as_ref().map_or(self.output.len(),|s|s.lines().len());
        if up {self.scroll=(self.scroll+1).min(len.saturating_sub(1));}
        else {self.scroll=self.scroll.saturating_sub(1);}
    }
    pub fn draw(&self,out:&mut impl Write,top:u16,rows:u16,width:u16,directory:&Path,prefs:&super::app_settings::Preferences)->io::Result<()> {
        let width=usize::from(width);
        let header=format!("[Hide] Terminal / {}{} | OS permissions (not scoped)",if self.interactive(){"PTY"}else{"Bash"},if self.focused{" *"}else{""});
        queue!(out,MoveTo(0,top),SetForegroundColor(prefs.accent),Print(super::clip(&header,width)),SetForegroundColor(prefs.foreground))?;
        if width>=84 {queue!(out,MoveTo((width-8) as u16,top),SetForegroundColor(prefs.accent),Print("[Resize]"),SetForegroundColor(prefs.foreground))?;}
        let directory=self.job.as_ref().map_or(directory,|job|job.directory.as_path());
        let cwd=if let Some(shell)=&self.shell {format!("Started in: {} | Alt+T focus | Alt+S close",terminal_tree::display_name(shell.initial_directory().as_os_str()))}else{format!("cwd: {}",terminal_tree::display_name(directory.as_os_str()))};
        queue!(out,MoveTo(0,top+1),Print(super::clip(&cwd,width)))?;
        let live;
        let lines=if let Some(shell)=&self.shell{live=shell.lines();&live}else if let Some(job)=&self.job {
            let mut current=self.output.clone();
            current.push(format!("$ {}",visible(job.command.as_bytes())));
            current.extend(job.bytes.split(|&b|b==b'\n').filter(|line|!line.is_empty()).map(visible));
            live=current;
            &live
        }else{&self.output};
        let capacity=usize::from(rows.saturating_sub(3));
        let end=lines.len().saturating_sub(self.scroll.min(lines.len()));
        let start=end.saturating_sub(capacity);
        for (row,line) in lines[start..end].iter().enumerate(){
            queue!(out,MoveTo(0,top+2+row as u16),Print(super::clip(line,width)))?;
        }
        let prompt=if self.interactive(){"Interactive Bash (TERM=dumb) | Alt+T file manager | Alt+S close".into()}
        else if self.running(){"Running Bash... Ctrl+C to stop".into()}
        else if self.focused{self.prompt_text(width)}
        else {"$ (click or Alt+T to type; Alt+S interactive; Alt+H hide)".into()};
        queue!(out,MoveTo(0,top+rows-1))?;
        if self.focused {queue!(out,SetBackgroundColor(prefs.selection))?;}
        queue!(out,Print(super::clip(&prompt,width)),SetBackgroundColor(prefs.background))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_unicode_and_preserve_scrollback(){
        let mut panel=Panel::default();
        let dir=Path::new("/tmp");
        panel.replace_input("abไทย".into());
        for key in [KeyCode::Left,KeyCode::Backspace,KeyCode::Home,KeyCode::Char('X'),KeyCode::Delete] {
            panel.input(KeyEvent::new(key,KeyModifiers::NONE),dir).unwrap();
        }
        assert_eq!(panel.input,"Xbไย");
        panel.output=vec!["x".repeat(1000);3000];
        panel.trim_scrollback();
        assert!(panel.output.iter().map(String::len).sum::<usize>()<=SCROLLBACK_LIMIT);
    }
    #[test]
    fn bash_runs_in_selected_directory_and_escapes_output(){
        let mut panel=Panel::default();
        panel.replace_input("printf 'panel-marker\\n\\033[31m'; pwd".into());
        panel.input(KeyEvent::new(KeyCode::Enter,KeyModifiers::NONE),Path::new("/tmp")).unwrap();
        for _ in 0..100 {
            if panel.tick().unwrap(){break;}
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!panel.running());
        assert!(panel.output.iter().any(|s|s=="panel-marker"));
        assert!(panel.output.iter().any(|s|s.contains("/tmp")));
        assert!(panel.output.iter().all(|s|!s.contains('\x1b')));
        panel.replace_input("printf second-marker".into());
        panel.input(KeyEvent::new(KeyCode::Enter,KeyModifiers::NONE),Path::new("/tmp")).unwrap();
        for _ in 0..100 {if panel.tick().unwrap(){break;}std::thread::sleep(Duration::from_millis(10));}
        assert!(panel.output.iter().any(|s|s=="panel-marker"));
        assert!(panel.output.iter().any(|s|s=="second-marker"));
    }
}
