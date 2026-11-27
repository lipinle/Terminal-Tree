//! Benchmark only: caller supplies a newly created synthetic fixture.
use std::path::Path;
use std::time::Instant;
use terminal_tree::operations::{Engine, Transfer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args=std::env::args_os().skip(1);
    let root=args.next().ok_or("missing synthetic workspace")?;
    let journal=args.next().ok_or("missing synthetic journal")?;
    let kind=args.next().ok_or("missing operation")?;
    if args.next().is_some() {return Err("unexpected argument".into());}
    let root=Path::new(&root);
    let kind=match kind.to_str() {Some("copy")=>Transfer::Copy,Some("move")=>Transfer::Move,_=>return Err("invalid operation".into())};
    let start=Instant::now();
    let engine=Engine::new(root,Path::new(&journal))?;
    let review=engine.review(&root.join("source"))?;
    engine.transfer(&review,&root.join("destination"),kind)?;
    assert!(engine.pending()?.is_empty());
    println!("{}",start.elapsed().as_secs_f64()*1000.0);
    Ok(())
}
