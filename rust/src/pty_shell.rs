//! Persistent Linux PTY, opt-in. TERM=dumb transcript, not a VT emulator.
//! Child output is display data, never forwarded as host terminal controls.
use crossterm::event::{KeyCode,KeyEvent,KeyEventKind,KeyModifiers};
use rustix::{fs::{fcntl_getfl,fcntl_setfl,OFlags},pty::{OpenptFlags,openpt,grantpt,unlockpt,ioctl_tiocgptpeer},termios::{Winsize,tcsetwinsize}};
use std::{fs::File,io::{self,Read,Write},path::{Path,PathBuf},process::{Child,Command,Stdio}};
const OUTPUT_LIMIT:usize=128*1024;
const INPUT_LIMIT:usize=8192;
pub struct Shell {
    master:Option<File>,
    child:Child,
    input:Vec<u8>,
    transcript:Vec<u8>,
    initial_directory:PathBuf,
    size:(u16,u16),
}
impl Shell {
    pub fn start(directory:&Path,cols:u16,rows:u16)->io::Result<Self>{
        let fd=openpt(OpenptFlags::RDWR|OpenptFlags::NOCTTY|OpenptFlags::CLOEXEC)?;
        grantpt(&fd)?;
        unlockpt(&fd)?;
        let slave:File=ioctl_tiocgptpeer(&fd,OpenptFlags::RDWR|OpenptFlags::NOCTTY|OpenptFlags::CLOEXEC)?.into();
        let master:File=fd.into();
        let flags=fcntl_getfl(&master)?;
        fcntl_setfl(&master,flags|OFlags::NONBLOCK)?;
        let cols=cols.max(1);let rows=rows.max(1);
        tcsetwinsize(&master,Winsize{ws_row:rows,ws_col:cols,ws_xpixel:0,ws_ypixel:0})?;
        // setsid acquires the slave as controlling terminal; no unsafe pre_exec.
        // No shell script construction; startup files deliberately bypassed.
        let child=Command::new("/usr/bin/setsid")
            .args(["--ctty","--wait","/bin/bash","--noprofile","--norc","-i"])
            .env_remove("BASH_ENV").env_remove("ENV").env_remove("PROMPT_COMMAND")
            .env("TERM","dumb").env("PS1","tt$ ").env("PS2","> ").env("HISTFILE","/dev/null")
            .current_dir(directory).stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?)).stderr(Stdio::from(slave)).spawn()?;
        Ok(Self{master:Some(master),child,input:Vec::new(),transcript:Vec::new(),initial_directory:directory.to_path_buf(),size:(cols,rows)})
    }
    pub fn initial_directory(&self)->&Path{&self.initial_directory}
    pub fn resize(&mut self,cols:u16,rows:u16)->io::Result<()>{
        let size=(cols.max(1),rows.max(1));
        if self.size!=size{
            if let Some(master)=&self.master{tcsetwinsize(master,Winsize{ws_row:size.1,ws_col:size.0,ws_xpixel:0,ws_ypixel:0})?;}
            self.size=size;
        }
        Ok(())
    }
    fn append(&mut self,bytes:&[u8]){
        self.transcript.extend_from_slice(bytes);
        if self.transcript.len()>OUTPUT_LIMIT{
            let cut=self.transcript.len()-OUTPUT_LIMIT;
            self.transcript.drain(..cut);
        }
    }
    pub fn tick(&mut self)->io::Result<bool>{
        let mut buffer=[0;4096];
        for _ in 0..8 {
            let Some(master)=&mut self.master else{break;};
            match master.read(&mut buffer){
                Ok(0)=>break,
                Ok(n)=>self.append(&buffer[..n]),
                Err(e) if e.kind()==io::ErrorKind::WouldBlock||e.raw_os_error()==Some(5)=>break,
                Err(e) if e.kind()==io::ErrorKind::Interrupted=>continue,
                Err(e)=>return Err(e),
            }
        }
        if let Some(master)=&mut self.master{
            if !self.input.is_empty(){
                match master.write(&self.input){
                    Ok(n)=>{self.input.drain(..n);}
                    Err(e) if e.kind()==io::ErrorKind::WouldBlock||e.kind()==io::ErrorKind::Interrupted||e.raw_os_error()==Some(5)=>(),
                    Err(e)=>return Err(e),
                }
            }
        }
        Ok(self.child.try_wait()?.is_some())
    }
    pub fn key(&mut self,k:KeyEvent)->io::Result<()>{
        if !matches!(k.kind,KeyEventKind::Press|KeyEventKind::Repeat){return Ok(());}
        let mut bytes=Vec::new();
        if k.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char(c)=k.code{
                if c.is_ascii_alphabetic(){bytes.push(c.to_ascii_lowercase() as u8-b'a'+1);}
            }
        }else{
            if k.modifiers.contains(KeyModifiers::ALT){bytes.push(27);}
            match k.code{
                KeyCode::Char(c) if !c.is_control()=>{let mut b=[0;4];bytes.extend_from_slice(c.encode_utf8(&mut b).as_bytes());}
                KeyCode::Enter=>bytes.push(b'\r'),KeyCode::Backspace=>bytes.push(127),
                KeyCode::Tab=>bytes.push(b'\t'),KeyCode::Esc=>bytes.push(27),
                KeyCode::Up=>bytes.extend_from_slice(b"\x1b[A"),KeyCode::Down=>bytes.extend_from_slice(b"\x1b[B"),
                KeyCode::Right=>bytes.extend_from_slice(b"\x1b[C"),KeyCode::Left=>bytes.extend_from_slice(b"\x1b[D"),
                KeyCode::Home=>bytes.extend_from_slice(b"\x1b[H"),KeyCode::End=>bytes.extend_from_slice(b"\x1b[F"),
                KeyCode::Delete=>bytes.extend_from_slice(b"\x1b[3~"),_=>(),
            }
        }
        if self.input.len()+bytes.len()>INPUT_LIMIT{return Err(io::Error::other("PTY input queue full"));}
        self.input.extend(bytes);
        Ok(())
    }
    pub fn styled_lines(&self)->Vec<super::shell_text::Line>{super::shell_text::parse(&self.transcript)}
    #[cfg(test)]
    pub fn lines(&self)->Vec<String>{self.styled_lines().iter().map(|l|l.text()).collect()}

}
impl Drop for Shell {
    fn drop(&mut self){
        // Closing the controlling PTY signals its foreground group. Reap
        // the direct child; detached descendants are not sandboxed.
        self.master.take();
        if matches!(self.child.try_wait(),Ok(None)){let _=self.child.kill();let _=self.child.wait();}
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn send(shell:&mut Shell,text:&str){
        for c in text.chars(){shell.key(KeyEvent::new(if c=='\n'{KeyCode::Enter}else{KeyCode::Char(c)},KeyModifiers::NONE)).unwrap();}
    }
    fn wait(shell:&mut Shell,marker:&str){
        for _ in 0..250{
            shell.tick().unwrap();
            if shell.lines().iter().any(|s|s==marker){return;}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("PTY did not show expected output: {:?}",shell.lines());
    }
    #[test]
    fn persistent_shell_cwd_env_and_tty(){
        let mut s=Shell::start(Path::new("/tmp"),80,12).unwrap();
        send(&mut s,"export TT_TEST=retained; cd /\n");
        send(&mut s,"test -t 0 && printf 'state:%s:%s\\n' \"$TT_TEST\" \"$PWD\"\n");
        wait(&mut s,"state:retained:/");
        s.resize(72,15).unwrap();
        send(&mut s,"stty size\n");wait(&mut s,"15 72");
        send(&mut s,"printf '\\033]0;unsafe\\007\\n'\n");
        for _ in 0..20{s.tick().unwrap();std::thread::sleep(std::time::Duration::from_millis(10));}
        assert!(s.lines().iter().all(|l|!l.contains('\x1b')&&!l.contains('\x07')));
    }
}
