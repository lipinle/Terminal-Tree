//! Git manager: fixed actions and explicit confirmation; no shell strings.
use std::{io,path::Path};
use terminal_tree::{display_name,git_manager::{Repo,Change},external_tools::Tool};

fn review(title:&str,detail:&str)->io::Result<bool>{
    Ok(super::choose(title,&["Cancel".into(),detail.into()])?==Some(1))
}
fn show_result(title:&str,result:io::Result<()>)->io::Result<()>{
    let line=match result{Ok(())=>"Completed. Git status will refresh.".into(),Err(e)=>format!("Git did not report success: {}. Check status before retrying.",display_name(std::ffi::OsStr::new(&e.to_string())))};
    super::page(title,&[line])
}
fn changes(repo:&Repo,editing:bool)->io::Result<()>{
    loop{
        let changes=repo.status()?;
        if changes.is_empty(){return super::page("Changes",&["No changes. Working tree is clean.".into()]);}
        let labels:Vec<_>=changes.iter().map(Change::label).collect();
        let Some(i)=super::choose("Changes | left=staged, right=working tree",&labels)? else{return Ok(());};
        let c=&changes[i];
        let mut labels=vec!["View changes not staged (Diff)".into(),"View staged changes (Diff)".into()];
        if editing{labels.extend(["Include this file in next commit (Stage)".into(),"Remove file from next commit (Unstage)".into()]);}
        match super::choose(&c.label(),&labels)?{
            Some(0)=>super::page("Working tree diff",&repo.diff(c,false)?)?,
            Some(1)=>super::page("Staged diff",&repo.diff(c,true)?)?,
            Some(i @ 2..=3) if editing=>{
                let action=if i==2{"Stage"}else{"Unstage"};
                if review(&format!("{action} this file?"),&c.label())?{show_result(action,repo.stage(c,i==2))?;}
            }
            _=>(),
        }
    }
}
pub fn show(directory:&Path,enabled:bool)->io::Result<()>{
    if !enabled{return super::page("Git disabled",&["Enable Git in Add-ons.".into()]);}
    if !Tool::Git.installed(){return super::page("Git unavailable",&["Install Git using your distribution package manager.".into()]);}
    if !review("Trusted repository only: Git config and filters may run helpers","I trust the repository; open Git Manager")?{return Ok(());}
    let repo=Repo::discover(directory)?;
    let mut editing=false;
    loop {
        let status=repo.status()?;
        let staged:Vec<_>=status.iter().filter(|c|c.staged()).cloned().collect();
        let unstaged=status.iter().filter(|c|c.unstaged()).count();
        let branch=repo.branch()?.into_iter().next().unwrap_or_default();
        let title=format!("Git Manager | {branch} | Staged {} | Working {unstaged}",staged.len());
        let labels=vec![
            format!("Review changes / choose files ({})",status.len()),
            "View commit history".into(),"View local branches".into(),"Review next commit".into(),
            format!("{} local Git editing",if editing{"Disable"}else{"Enable"}),
            if editing{"Save staged changes (Commit)".into()}else{"Save staged changes (editing disabled)".into()},
            if editing{"Create local branch (stay on current branch)".into()}else{"Create local branch (editing disabled)".into()},
            "Refresh status".into(), "Help / Git vocabulary".into(),
        ];
        match super::choose(&title,&labels)?{
            None=>return Ok(()),
            Some(0)=>changes(&repo,editing)?,
            Some(1)=>match repo.history(){Ok(v)=>super::page("Commit history",&v)?,Err(_)=>super::page("Commit history",&["No readable commit history yet.".into()])?},
            Some(2)=>super::page("Local branches",&repo.branches()?)?,
            Some(3)=>super::page("Next commit / staged summary",&repo.staged_summary()?)?,
            Some(4)=>{
                if editing{editing=false;}else{
                    let detail=format!("Enable Git editing in {}",display_name(repo.root().as_os_str()));
                    editing=review("Separate Git authority: repository filters may execute programs; outside file journal; hooks/signing disabled",&detail)?;
                }
            }
            Some(5) if editing=>{
                if staged.is_empty(){super::page("Commit",&["Stage a file first.".into()])?;continue;}
                let mut summary:Vec<_>=staged.iter().map(Change::label).collect();
                summary.extend(repo.staged_summary()?);
                super::page("Review all staged files before committing",&summary)?;
                if let Some(message)=super::workspace::prompt("Commit message (describe what changed):")?{
                    if !message.trim().is_empty()&&review("Create local commit from all staged files?",&message)?{show_result("Commit",repo.commit(&message,&staged))?;}
                }
            }
            Some(6) if editing=>{
                if let Some(name)=super::workspace::prompt("New branch name:")?{
                    if !name.is_empty()&&review("Create branch without switching?",&name)?{show_result("Create branch",repo.create_branch(&name))?;}
                }
            }
            Some(5|6)=>super::page("Editing disabled",&["Enable local Git editing in this menu first.".into()])?,
            Some(8)=>super::page("Git help",&[
                "Stage: choose changes for your next commit.".into(),"Unstage: remove changes from next commit; keep files.".into(),
                "Commit: save a local snapshot of staged files.".into(),"Branch: a separate line of development.".into(),
                "No push, pull, reset, checkout or force operations in this manager.".into(),
                "Editing requires local Git user.name and user.email; global config is not loaded.".into(),
                "Hooks and signing disabled. Use normal Git when project policy requires them.".into(),
                "Git commands are outside the file manager's journal and undo.".into(),
            ])?,
            _=>(),
        }
    }
}
