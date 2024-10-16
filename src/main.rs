#![feature(slice_split_once)]

use camino::Utf8PathBuf;
use clap::Parser;
use colored::{ColoredString, Colorize};
use gix::bstr::ByteSlice;
use gix::status::index_worktree::iter::{Item, RewriteSource};
use gix::status::plumbing::index_as_worktree::EntryStatus;
use gix::submodule::config::Ignore;
use gix::Url;
use itertools::Itertools;
use std::process::Command;
use tracing::debug;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    cwd: Option<Utf8PathBuf>,
    #[clap(subcommand)]
    command: Subcommand,
}

#[derive(Parser)]
enum Subcommand {
    /// Clone a repository with submodules
    Clone {
        url: String,
        path: Option<Utf8PathBuf>,
    },
    /// Remove a submodule
    Rm { path: Utf8PathBuf },
    /// Initialize submodules
    Init,
    /// List submodules
    Ls,
    /// Show submodules and their changed files
    Status,
}

// Tries to use the last component of the path as the name of the submodule.
// If that fails, it uses the full path.
fn format_name(name: &str) -> &str {
    name.rsplit_once('/')
        .or(name.rsplit_once('\\'))
        .map(|(_, name)| name)
        .unwrap_or(name)
}

fn display_name(submodule: &gix::Submodule) -> Result<ColoredString, anyhow::Error> {
    let state = submodule.state()?;
    if state.repository_exists {
        Ok(format_name(&submodule.name().to_str_lossy()).blue().bold())
    } else {
        Ok(format_name(&submodule.name().to_str_lossy()).bold())
    }
}

fn display_change(change: &Item) -> Result<(), anyhow::Error> {
    match change {
        Item::Modification {
            rela_path, status, ..
        } => {
            let name = match status {
                EntryStatus::Conflict(_) => rela_path.to_str_lossy().bold().red(),
                EntryStatus::Change(_) | EntryStatus::IntentToAdd => {
                    rela_path.to_str_lossy().bold().yellow()
                }
                EntryStatus::NeedsUpdate(_) => return Ok(()),
            };
            println!("    {}", name);
        }
        Item::DirectoryContents { entry, .. } => {
            // We're assuming it's untracked
            println!("    {}", entry.rela_path.to_str_lossy().red());
        }
        Item::Rewrite {
            source:
                RewriteSource::RewriteFromIndex {
                    source_rela_path, ..
                },
            dirwalk_entry,
            ..
        } => {
            println!(
                "    {} -> {}",
                source_rela_path.to_str_lossy().bold(),
                dirwalk_entry.rela_path.to_str_lossy().bold()
            );
        }
        Item::Rewrite {
            source:
                RewriteSource::CopyFromDirectoryEntry {
                    source_dirwalk_entry,
                    ..
                },
            dirwalk_entry,
            ..
        } => {
            println!(
                "    {} -> {}",
                source_dirwalk_entry.rela_path.to_str_lossy().bold(),
                dirwalk_entry.rela_path.to_str_lossy().bold()
            );
        }
    }

    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    let cwd = if let Some(cwd) = args.cwd {
        cwd
    } else {
        std::env::current_dir()?.try_into()?
    };

    match args.command {
        Subcommand::Clone { url, path } => {
            let git = which::which("git")?;
            let mut command = Command::new(git);

            command
                .arg("clone")
                .arg("--recursive")
                .arg(&url)
                .current_dir(&cwd);

            if let Some(path) = &path {
                command.arg(path);
            }

            let status = command.spawn()?.wait()?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }

            let url = Url::try_from(url.as_str())?;
            let repo_path = match path {
                Some(path) => path,
                None if url.host() == Some("github.com") => {
                    let url_path = url.path.to_string();
                    let Some((_, path)) = url_path.rsplit_once('/') else {
                        return Ok(());
                    };
                    let path = path.trim_end_matches(".git");
                    Utf8PathBuf::from(path)
                }
                None => {
                    debug!("cannot determine path from url");
                    return Ok(());
                }
            };

            let abs_repo_path = cwd.join(repo_path);

            let repo = gix::discover(abs_repo_path)?;
            let Some(submodules) = repo.submodules()? else {
                return Ok(());
            };
            for submodule in submodules.sorted_by(|a, b| a.name().cmp(b.name())) {
                println!(
                    "{} {} {} {}",
                    "initialized".bold(),
                    display_name(&submodule)?,
                    "at".bold(),
                    submodule.path()?.to_str_lossy().dimmed().bold()
                );
            }
        }
        Subcommand::Init => {
            let git = which::which("git")?;
            Command::new(&git)
                .arg("submodule")
                .arg("init")
                .current_dir(&cwd)
                .spawn()?
                .wait()?;

            Command::new(&git)
                .arg("submodule")
                .arg("update")
                .current_dir(&cwd)
                .spawn()?
                .wait()?;
        }
        Subcommand::Rm { path } => {
            let git = which::which("git")?;
            Command::new(&git)
                .arg("rm")
                .arg(path)
                .current_dir(&cwd)
                .spawn()?
                .wait()?;
        }
        Subcommand::Ls => {
            let repo = gix::discover(cwd)?;
            let Some(submodules) = repo.submodules()? else {
                println!("No submodules found");
                return Ok(());
            };
            for submodule in submodules.sorted_by(|a, b| a.name().cmp(b.name())) {
                println!(
                    "{} {}",
                    display_name(&submodule)?,
                    submodule.path()?.to_str_lossy().dimmed()
                );
            }
        }
        Subcommand::Status => {
            let repo = gix::discover(cwd)?;
            let Some(submodules) = repo.submodules()? else {
                println!("No submodules found");
                return Ok(());
            };
            for submodule in submodules.sorted_by(|a, b| a.name().cmp(b.name())) {
                let status = submodule.status(Ignore::None, false)?;
                println!(
                    "{} {} {}",
                    display_name(&submodule)?,
                    submodule.path()?.to_str_lossy().dimmed(),
                    match status.is_dirty() {
                        Some(true) => "dirty".yellow().bold(),
                        Some(false) => "clean".green().bold(),
                        None if submodule.state()?.repository_exists => "unknown".bold(),
                        None => "uninitialized".dimmed().bold(),
                    }
                );
                if let Some(changes) = status.changes {
                    if !changes.is_empty() {
                        println!("  changes:");
                    }

                    for change in changes {
                        display_change(&change)?;
                    }
                }
            }
        }
    }

    Ok(())
}
