//! Display only: journal paths are untrusted data, never action inputs.
use std::{ffi::OsString,os::unix::ffi::OsStringExt};
use serde_json::Value;
use terminal_tree::display_name;

fn field(record:&Value,key:&str)->String{
    let hexkey=format!("{key}_bytes_hex");
    if let Some(s)=record.get(&hexkey).and_then(Value::as_str){
        if s.len()%2!=0||!s.bytes().all(|b|b.is_ascii_hexdigit()){return "[invalid path encoding]".into();}
        let bytes:Vec<_>=(0..s.len()).step_by(2).map(|i|u8::from_str_radix(&s[i..i+2],16).unwrap()).collect();
        if bytes.contains(&0){return "[invalid NUL path]".into();}
        return display_name(&OsString::from_vec(bytes));
    }
    record.get(key).and_then(Value::as_str).map(|s|display_name(s.as_ref())).unwrap_or_else(||"-".into())
}
pub fn show(records:&[Value])->std::io::Result<()>{
    let mut lines=vec![format!("Pending records: {}",records.len()),
        "Pending does not mean failed. A write may already have completed.".into(),
        "Inspect original, destination and staging/metadata before retrying.".into(),
        "Read-only view; no automatic deletion, rollback or replay.".into()];
    for r in records{
        lines.push(String::new());
        lines.push(format!("ID: {} | Operation: {}",field(r,"id"),field(r,"operation")));
        for key in ["source","destination","stage"]{lines.push(format!("{key}: {}",field(r,key)));}
    }
    use unicode_width::UnicodeWidthChar;
    let width=usize::from(crossterm::terminal::size()?.0).saturating_sub(1).max(1);
    let mut wrapped=Vec::new();
    for line in lines{
        let mut part=String::new();let mut used=0;
        for c in line.chars(){
            let n=c.width().unwrap_or(0);
            if used+n>width&&!part.is_empty(){wrapped.push(std::mem::take(&mut part));used=0;}
            part.push(c);used+=n;
        }
        wrapped.push(part);
    }
    super::page("Recovery - pending journal",&wrapped)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn raw_paths_and_controls_are_safe(){
        let r=serde_json::json!({"source":null,"source_bytes_hex":"2f746d702fff1b0a"});
        let s=field(&r,"source");assert!(s.contains("\\xff"));assert!(!s.contains('\x1b'));assert!(!s.contains('\n'));
        assert_eq!(field(&serde_json::json!({"source_bytes_hex":"zz"}),"source"),"[invalid path encoding]");
    }
}
