use repo_stealer_core::{compare, RefChange, RefSnapshot};
use std::{env, fs::File, io::Read, process::ExitCode};
mod git;
mod migration;
mod vault;

// Bound input allocation. This prototype handles fixture snapshots, not bulk imports.
fn read_snapshot(path: &std::ffi::OsStr) -> Result<RefSnapshot, String> {
    const MAX: u64 = 16 * 1024 * 1024;
    let mut text = String::new();
    File::open(path)
        .map_err(|_| "cannot open snapshot")?
        .take(MAX + 1)
        .read_to_string(&mut text)
        .map_err(|_| "cannot read UTF-8 snapshot")?;
    if text.len() as u64 > MAX {
        return Err("snapshot exceeds 16 MiB limit".into());
    }
    RefSnapshot::parse(&text)
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "migration") {
        return migration::run(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "workspace") {
        return git::run_workspace(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "git") {
        return git::run(&args[1..]);
    }
    if args.len() == 1 && args[0] == "--version" {
        println!("repo-stealer {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "vault") {
        return vault::run(&args[1..]);
    }
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{}\n\n{}\n\n{}", vault::HELP, git::HELP, migration::HELP);
        return Ok(());
    }
    if args.len() != 3 || args[0] != "plan" {
        return Err("usage: repo-stealer plan SOURCE.refs DESTINATION.refs".into());
    }
    let plan = compare(&read_snapshot(&args[1])?, &read_snapshot(&args[2])?)?;
    println!("READ-ONLY PREVIEW — heads/tags only; no operation is authorized");
    for item in plan {
        let label = match item.change {
            RefChange::CreateCandidate => "CREATE CANDIDATE",
            RefChange::Unchanged => "UNCHANGED",
            RefChange::NeedsAncestryCheck => "CHANGED / CHECK ANCESTRY",
            RefChange::DestinationOnlyPreserve => "DESTINATION ONLY / PRESERVE",
        };
        println!("{label}\t{}", item.name);
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
