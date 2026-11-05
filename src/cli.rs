use std::fs;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::git::Git;
use crate::key::Key;
use crate::status;

/// fjo - hermes-compatible git/ssh helper CLI.
#[derive(Parser)]
#[command(name = "fjo", version, about = "hermes-compatible git/ssh helper")]
pub struct Cli {
    /// Emit a single JSON result object instead of human text
    #[arg(long, global = true)]
    pub json: bool,
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
    /// Report key presence and toolchain availability (agent preflight)
    Status,
}

/// What a finished command wants to report: a human message plus an
/// optional extra payload that only shows up in --json output.
#[derive(Serialize)]
pub struct Outcome {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

impl Cli {
    /// Entry point: parse errors bubble up as anyhow errors with exit 1.
    ///
    /// Never prompts, never reads stdin: failures are errors, not questions.
    pub fn run(self) -> Result<Outcome> {
        match self.command {
            Cmd::Setup { name, force } => Self::setup(&name, force),
            Cmd::Clone { repo, dest, key, password } => {
                Self::clone(&repo, dest.as_deref(), key.as_deref(), &password)
            }
            Cmd::Status => Self::status_cmd(),
        }
    }

    /// Generate a new passwordless ssh key under {cwd}/.ssh.
    fn setup(name: &str, force: bool) -> Result<Outcome> {
        let key = Key::new(name)?;
        key.generate(force)?;

        let pub_key =
            fs::read_to_string(&key.pub_key)
                .with_context(|| format!("key generated but {} unreadable", key.pub_key.display()))?;

        Ok(Outcome {
            message: format!(
                "key written: {}\npublic key:\n{}",
                key.key.display(),
                pub_key.trim()
            ),
            extra: Some(serde_json::json!({
                "key_path": key.key.display().to_string(),
                "pub_path": key.pub_key.display().to_string(),
                "public_key": pub_key.trim(),
            })),
        })
    }

    /// Clone `repo` using the key auto-detected under {cwd}/.ssh.
    ///
    /// Non-interactive by design: an empty password keeps ssh in batch mode
    /// (no prompts ever), while a provided password is fed via sshpass.
    fn clone(repo: &str, dest: Option<&str>, key: Option<&str>, password: &str) -> Result<Outcome> {
        let key = Key::pick(key)?;
        Git::clone(repo, dest, &key, password)?;
        Ok(Outcome {
            message: format!("cloned {repo}"),
            extra: Some(serde_json::json!({ "key_path": key.key.display().to_string() })),
        })
    }

    /// Preflight snapshot for agents.
    fn status_cmd() -> Result<Outcome> {
        let st = status::status();
        let message = format!(
            "keys in {}: [{}]; key resolvable: {}; git: {}; ssh-keygen: {}; sshpass: {}",
            st.keys_dir,
            st.keys.join(", "),
            st.key_resolvable,
            st.git_ok,
            st.ssh_keygen_ok,
            st.sshpass_ok,
        );
        let extra = serde_json::to_value(&st).ok();
        Ok(Outcome { message, extra })
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
        Cli::try_parse_from([
            "fjo", "clone", "git@github.com:user/repo.git",
        ])
        .expect("clone should parse");
        Cli::try_parse_from(["fjo", "setup", "--name", "k", "--force"]).expect("setup should parse");
        Cli::try_parse_from(["fjo", "status"]).expect("status should parse");
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

    #[test]
    fn json_flag_is_global() {
        let cli = Cli::try_parse_from(["fjo", "--json", "clone", "git@github.com:user/r.git"])
            .expect("global --json should parse");
        assert!(cli.json);
        let cli = Cli::try_parse_from(["fjo", "clone", "--json", "git@github.com:user/r.git"])
            .expect("--json after subcommand should parse");
        assert!(cli.json);
    }

    /// The manifest is the agent-facing contract; drift silently breaks
    /// agents, so version and command surface are pinned by tests.
    #[test]
    fn hermes_manifest_matches_code() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("hermes.json");
        let raw =
            fs::read_to_string(&path).expect("hermes.json must sit next to Cargo.toml");
        let manifest: serde_json::Value = serde_json::from_str(&raw).expect("manifest must be valid json");

        assert_eq!(manifest["name"], "fjo");
        assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(manifest["non_interactive"], true);

        let commands: Vec<&str> = manifest["commands"]
            .as_array()
            .expect("commands array")
            .iter()
            .filter_map(|c| c["name"].as_str())
            .collect();
        for expected in ["setup", "clone", "status"] {
            assert!(
                commands.contains(&expected),
                "manifest must declare the `{expected}` command"
            );
        }

        let globals: Vec<&str> = manifest["globals"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|c| c["arg"].as_str()).collect())
            .unwrap_or_default();
        assert!(
            globals.iter().any(|g| g.contains("--json")),
            "manifest must declare the --json global"
        );
    }
}
