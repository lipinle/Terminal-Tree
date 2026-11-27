//! Benchmark only: caller supplies a newly created synthetic fixture.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;
use terminal_tree::operations::{Engine, Transfer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = args.next().ok_or("missing synthetic workspace")?;
    let journal = args.next().ok_or("missing synthetic journal")?;
    let kind = args.next().ok_or("missing operation")?;
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let root = Path::new(&root);
    let kind = match kind.to_str() {
        Some("copy") => Transfer::Copy,
        Some("move") => Transfer::Move,
        _ => return Err("invalid operation".into()),
    };
    let start = Instant::now();
    let engine = Engine::new(root, Path::new(&journal))?;
    let mut phase_start = Instant::now();
    let mut phases = BTreeMap::new();
    let review = engine.review(&root.join("source"))?;
    phases.insert(
        "initial_review",
        phase_start.elapsed().as_secs_f64() * 1000.0,
    );
    let target = engine.destination(&root.join("destination"))?;
    phase_start = Instant::now();
    let mut phase = "setup";
    engine.transfer_reviewed_with(&review, &target, kind, &mut |point| {
        let next = match point {
            "transfer-prepare" => "journal_and_staging",
            "copy-entry" | "copy-partial" | "copy-reflink" => "copy_payload",
            "copy-verify-stage" => "verify_stage_and_sync",
            "copy-verify-source" | "move-verify-source" => "verify_source",
            "copy-before-publish" => "pre_publish_checks",
            "transfer-commit" => "rename_and_sync",
            "copy-after-sync" | "move-after-sync" => "finish_journal",
            "copy-done" | "move-done" => "after_commit",
            "move-verify-result" => "verify_moved_result",
            _ => return Ok(()),
        };
        if phase != next {
            *phases.entry(phase).or_insert(0.0) += phase_start.elapsed().as_secs_f64() * 1000.0;
            phase_start = Instant::now();
            phase = next;
        }
        Ok(())
    })?;
    *phases.entry(phase).or_insert(0.0) += phase_start.elapsed().as_secs_f64() * 1000.0;
    eprintln!("TT_PHASES {}", serde_json::to_string(&phases)?);
    assert!(engine.pending()?.is_empty());
    println!("{}", start.elapsed().as_secs_f64() * 1000.0);
    Ok(())
}
