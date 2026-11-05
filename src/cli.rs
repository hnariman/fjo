use std::fs;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::git::Git;
use crate::key::Key;

/// fjo - hermes-compatible git/ssh helper CLI.
#[derive(Parser)]
#[command(name = "fjo", version, about = "hermes-compatible git/ssh helper")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Generate a fresh passwordless ssh key in {cwd}/.ssh/<key_name>
    Setup {
        /// Name for the new key
        #[arg(long, short)]
        name: String,
        /// Overwrite existing key files instead of failing
        #[arg(long, short)]
        force: bool,
    },
    /// Clone a repo using the key found in {cwd}/.ssh/
    Clone {
        /// Repository url, e.g. git@github.com:user/repo.git
        repo: String,
        /// Target directory (git decides the default if omitted)
        dest: Option<String>,
        /// Explicit key name under {cwd}/.ssh (overrides auto-detection)
        #[arg(long, short)]
        key: Option<String>,
        /// Password fed to ssh non-interactively; empty means no prompt at all
        #[arg(long, short, default_value = "")]
        password: String,
    },
}

impl Cli {
    /// Entry point: parse errors bubble up as anyhow errors with exit 1.
    pub fn run(self) -> Result<()> {
        match self.command {
            Cmd::Setup { name, force } => Self::setup(&name, force),
            Cmd::Clone { repo, dest, key, password } => {
                Self::clone(&repo, dest.as_deref(), key.as_deref(), &password)
            }
        }
    }

    /// Generate a new passwordless ssh key under {cwd}/.ssh and print its public half.
    fn setup(name: &str, force: bool) -> Result<()> {
        let key = Key::new(name)?;
        key.generate(force)?;

        let pub_key =
            fs::read_to_string(&key.pub_key)
                .with_context(|| format!("key generated but {} unreadable", key.pub_key.display()))?;

        println!("key written: {}", key.key.display());
        println!("public key:");
        println!("{}", pub_key.trim());
        Ok(())
    }

    /// Clone `repo` using the key auto-detected under {cwd}/.ssh.
    ///
    /// Non-interactive by design: an empty password keeps ssh in batch mode
    /// (no prompts ever), while a provided password is fed via sshpass.
    fn clone(repo: &str, dest: Option<&str>, key: Option<&str>, password: &str) -> Result<()> {
        let key = Key::pick(key)?;
        Git::clone(repo, dest, &key, password)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_requires_name() {
        assert!(
            Cli::try_parse_from(["fjo", "setup"]).is_err(),
            "setup without --name must fail, agents should not be prompted"
        );
    }

    #[test]
    fn cli_parses() {
        Cli::try_parse_from(["fjo", "clone", "git@github.com:user/repo.git"])
            .expect("clone should parse");
        Cli::try_parse_from(["fjo", "setup", "--name", "k", "--force"]).expect("setup should parse");
    }

    #[test]
    fn clone_password_defaults_to_empty() {
        let cli = Cli::try_parse_from(["fjo", "clone", "git@github.com:user/repo.git"])
            .expect("clone should parse");
        match cli.command {
            Cmd::Clone { password, .. } => {
                assert!(password.is_empty(), "default password must be an empty string");
            }
            _ => panic!("expected clone"),
        }
        let cli = Cli::try_parse_from([
            "fjo", "clone", "--password", "hunter2", "git@github.com:user/repo.git",
        ])
        .expect("clone with --password should parse");
        match cli.command {
            Cmd::Clone { password, .. } => assert_eq!(password, "hunter2"),
            _ => panic!("expected clone"),
        }
    }
}
