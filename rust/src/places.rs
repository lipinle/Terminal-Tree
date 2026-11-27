//! Read-only location picker; discovery never grants write permissions.
use std::{io,path::PathBuf};
#[cfg(target_os="linux")]
use std::{collections::BTreeSet,ffi::OsString,fs,path::Path};
#[cfg(target_os="linux")]
use std::os::unix::ffi::OsStringExt;
use terminal_tree::display_name;

#[cfg(target_os="linux")]
fn decode(raw:&[u8])->io::Result<PathBuf>{
    let mut out=Vec::new();let mut i=0;
    while i<raw.len(){
        if raw[i]==b'\\'{
            if i+3>=raw.len() || !raw[i+1..i+4].iter().all(|b|(b'0'..=b'7').contains(b)){return Err(io::Error::other("invalid mount escape"));}
            let n=u16::from(raw[i+1]-b'0')*64+u16::from(raw[i+2]-b'0')*8+u16::from(raw[i+3]-b'0');
            if n==0||n>255{return Err(io::Error::other("invalid mount byte"));}
            out.push(n as u8);i+=4;
        }else{out.push(raw[i]);i+=1;}
    }
    let path=PathBuf::from(OsString::from_vec(out));
    if !path.is_absolute(){return Err(io::Error::other("mount path not absolute"));}
    Ok(path)
}
#[cfg(target_os="linux")]
fn media_mounts(bytes:&[u8])->io::Result<Vec<PathBuf>>{
    let mut out=BTreeSet::new();
    for line in bytes.split(|&b|b==b'\n').filter(|l|!l.is_empty()){
        let fields:Vec<_>=line.split(|&b|b==b' ').collect();
        if fields.len()<10{return Err(io::Error::other("invalid mountinfo"));}
        let p=decode(fields[4])?;
        if ["/mnt","/media","/run/media"].iter().any(|root|p.starts_with(root)&&p!=Path::new(root)){
            out.insert(p);
        }
    }
    Ok(out.into_iter().collect())
}
pub fn choose()->io::Result<Option<PathBuf>>{
    let mut entries=Vec::new();
    if let Some(home)=super::platform::home_dir(){
        entries.push((format!("Home | {}",display_name(home.as_os_str())),home.clone()));
        #[cfg(target_os="linux")]
        {
        let data=std::env::var_os("XDG_DATA_HOME").filter(|v|!v.is_empty()).map(PathBuf::from).unwrap_or_else(||home.join(".local/share"));
        let trash=data.join("Trash/files");
        if trash.is_absolute() && trash.is_dir(){entries.push((format!("Trash | {}",display_name(trash.as_os_str())),trash));}
        }
    }
    let root=super::platform::filesystem_root()?;
    entries.push((format!("File System | {}",display_name(root.as_os_str())),root));
    // Missing procfs (minimal containers/chroots) must not prevent basic
    // browsing. The safety policy still requires mount information for writes.
    #[cfg(target_os="linux")]
    let detected=fs::read("/proc/self/mountinfo").and_then(|bytes|media_mounts(&bytes));
    #[cfg(target_os="linux")]
    let title=match detected{
        Ok(paths)=>{
            for p in paths{entries.push((format!("Mounted device | {}",display_name(p.as_os_str())),p));}
            "My Computer - browsing does not approve writes"
        },
        Err(_)=>"My Computer - mount discovery unavailable",
    };
    #[cfg(not(target_os="linux"))]
    let title="My Computer - read-only platform preview";
    let labels:Vec<_>=entries.iter().map(|(s,_)|s.clone()).collect();
    Ok(super::choose(title,&labels)?.map(|i|entries[i].1.clone()))
}
#[cfg(all(test,target_os="linux"))]
mod tests{
    use super::*;
    #[test]
    fn mount_paths_are_decoded_without_granting_access(){
        let sample=b"1 0 8:1 / / rw - ext4 /dev/sda rw\n2 1 8:2 / /media/a\\040b rw - ext4 /dev/sdb rw\n3 1 0:1 / /proc rw - proc proc rw\n4 1 8:2 / /mnt/raw\\377 rw - ext4 /dev/sdb rw\n";
        let p=media_mounts(sample).unwrap();
        assert_eq!(p,vec![PathBuf::from("/media/a b"),PathBuf::from(OsString::from_vec(b"/mnt/raw\xff".to_vec()))]);
        assert!(decode(b"/mnt/\\000").is_err());
    }
}
