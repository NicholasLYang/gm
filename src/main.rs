#![feature(slice_split_once)]

use camino::Utf8PathBuf;
use clap::Parser;
use colored::Colorize;
use gix::bstr::ByteSlice;
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
    // TODO: Print the submodules in nice colors
    Clone {
        url: String,
        path: Option<Utf8PathBuf>,
    },
    Ls,
}

// Tries to use the last component of the path as the name of the submodule.
// If that fails, it uses the full path.
fn format_name(name: &str) -> &str {
    name.rsplit_once('/')
        .or(name.rsplit_once('\\'))
        .map(|(_, name)| name)
        .unwrap_or(name)
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
            let git = which::which("git").unwrap();
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
                let state = submodule.state()?;
                let name = if state.repository_exists {
                    format_name(&submodule.name().to_str_lossy()).blue().bold()
                } else {
                    format_name(&submodule.name().to_str_lossy()).bold()
                };
                println!(
                    "{} {} {} {}",
                    "initialized".bold(),
                    name,
                    "at".bold(),
                    submodule.path()?.to_str_lossy().dimmed().bold()
                );
            }
        }
        Subcommand::Ls => {
            let repo = gix::discover(cwd)?;
            let Some(submodules) = repo.submodules()? else {
                println!("No submodules found");
                return Ok(());
            };
            for submodule in submodules.sorted_by(|a, b| a.name().cmp(b.name())) {
                let state = submodule.state()?;
                let name = if state.repository_exists {
                    format_name(&submodule.name().to_str_lossy()).blue().bold()
                } else {
                    format_name(&submodule.name().to_str_lossy()).bold()
                };
                println!("{} {}", name, submodule.path()?.to_str_lossy().dimmed());
            }
        }
    }

    Ok(())
}
