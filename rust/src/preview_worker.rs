//! One in-flight decoder, one replaceable pending request. Never writes to the
//! terminal. Results from obsolete selections are discarded on the UI thread.
use std::{io,path::PathBuf,thread::{self,JoinHandle},time::{Duration,Instant}};
use terminal_tree::visual_preview::{self,Frame};
const DEBOUNCE:Duration=Duration::from_millis(120);

pub struct Request {
    pub path:PathBuf,pub name:String,pub page:u32,pub pdf:bool,
    pub hd:Option<(u32,u32)>,pub fallback:(u32,u32),
}
pub struct Completed {
    pub name:String,pub page:u32,pub pdf:bool,pub bitmap:bool,pub result:io::Result<Frame>,
}
fn decode(req:Request)->Completed{
    let hd=req.hd.and_then(|(w,h)|visual_preview::render_hd(&req.path,req.page,w,h).ok());
    let bitmap=hd.is_some();
    let result=match hd{
        Some(f)=>Ok(f),
        None=>visual_preview::render(&req.path,req.page,req.fallback.0,req.fallback.1),
    };
    Completed{name:req.name,page:req.page,pdf:req.pdf,bitmap,result}
}
#[derive(Default)]
pub struct Queue {
    generation:u64,
    pending:Option<(u64,Instant,Request)>,
    active:Option<(u64,JoinHandle<Completed>)>,
}
impl Queue{
    pub fn busy(&self)->bool{self.pending.is_some()||self.active.is_some()}
    pub fn cancel(&mut self){
        self.generation=self.generation.wrapping_add(1);
        self.pending=None;
        // A running helper has existing process limits; it is allowed to
        // finish, but may never publish its now-obsolete result.
    }
    pub fn request(&mut self,req:Request){
        self.cancel();
        self.pending=Some((self.generation,Instant::now()+DEBOUNCE,req));
    }
    pub fn poll(&mut self)->Option<Completed>{
        if self.active.as_ref().is_some_and(|(_,h)|h.is_finished()){
            let (id,handle)=self.active.take()?;
            let result=handle.join();
            if id==self.generation {
                return Some(result.unwrap_or_else(|_|Completed{name:"Visual Preview".into(),page:1,pdf:false,bitmap:false,result:Err(io::Error::other("preview worker stopped unexpectedly"))}));
            }
        }
        if self.active.is_none()&&self.pending.as_ref().is_some_and(|(_,at,_)|Instant::now()>=*at){
            let (id,_,req)=self.pending.take()?;
            let (name,page,pdf)=(req.name.clone(),req.page,req.pdf);
            match thread::Builder::new().name("tt-visual-preview".into()).spawn(move||decode(req)){
                Ok(handle)=>self.active=Some((id,handle)),
                Err(error)=>return Some(Completed{name,page,pdf,bitmap:false,result:Err(error)}),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    fn request(name:&str)->Request{Request{path:PathBuf::from("/missing"),name:name.into(),page:1,pdf:false,hd:None,fallback:(10,10)}}
    #[test]fn pending_requests_coalesce_and_cancel(){
        let mut q=Queue::default();
        q.request(request("first"));
        q.request(request("latest"));
        assert!(q.busy());assert!(q.active.is_none());
        assert_eq!(q.pending.as_ref().unwrap().2.name,"latest");
        assert!(q.poll().is_none());
        q.cancel();assert!(!q.busy());
    }
    #[test]fn obsolete_completion_cannot_publish(){
        let mut q=Queue::default();
        let handle=thread::spawn(||Completed{name:"obsolete".into(),page:1,pdf:false,bitmap:false,result:Err(io::Error::other("fixture"))});
        while !handle.is_finished(){thread::yield_now();}
        q.active=Some((q.generation,handle));
        q.cancel();
        assert!(q.poll().is_none());
        assert!(!q.busy());
    }
}
