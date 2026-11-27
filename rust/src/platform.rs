//! Desktop integration without a shell or a mandatory desktop environment.
use std::{io,path::Path,process::{Child,Command,Stdio}};
fn spawn(program:&str,args:&[&str],path:&Path)->io::Result<Child>{
    Command::new(program).args(args).arg(path)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
}
pub fn open_file(path:&Path)->io::Result<Child>{
    if !path.is_absolute(){return Err(io::Error::new(io::ErrorKind::InvalidInput,"absolute file path required"));}
    match spawn("xdg-open",&[],path){
        Err(e) if e.kind()==io::ErrorKind::NotFound=>match spawn("gio",&["open","--"],path){
            Err(e) if e.kind()==io::ErrorKind::NotFound=>Err(io::Error::new(io::ErrorKind::NotFound,"No desktop opener: install xdg-utils or GLib/GIO, or use file preview")),
            other=>other,
        },
        other=>other,
    }
}
#[cfg(test)]
mod tests{
    #[test]
    fn relative_path_is_not_passed_to_opener(){
        assert_eq!(super::open_file(std::path::Path::new("-option")).unwrap_err().kind(),std::io::ErrorKind::InvalidInput);
    }
}
