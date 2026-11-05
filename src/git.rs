use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::key::Key;

pub struct Git;

impl Git {
    /// Clone `repo`, authenticating with `key` via GIT_SSH_COMMAND.
    ///
    /// Never prompts: with an empty `password`, ssh runs in batch mode
    /// (`BatchMode=yes`, no password/passphrase prompt — fails fast instead);
    /// with a password, the ssh command is wrapped in `sshpass -p <password>`,
    /// which feeds the password non-interactively.
    pub fn clone(repo: &str, dest: Option<&str>, key: &Key, password: &str) -> Result<()> {
        let mut ssh = key.ssh_command();
        if password.is_empty() {
            ssh.push_str(" -o BatchMode=yes");
        }

        let mut cmd = Command::new("git");
        cmd.args(["clone", repo]);
        if let Some(dest) = dest {
            cmd.arg(dest);
        }
        if password.is_empty() {
            cmd.env("GIT_SSH_COMMAND", &ssh);
        } else {
            Self::require_sshpass().context("a password was given")?;
            ssh.insert_str(0, &format!("sshpass -p {password:?} "));
            cmd.env("GIT_SSH_COMMAND", &ssh);
        }

        let status = cmd.status().context("failed to run git")?;
        if !status.success() {
            bail!(
                "git clone failed ({}), check the url and that the key is registered on the remote",
                status.code().unwrap_or(-1)
            );
        }
        Ok(())
    }

    /// Bail cleanly when a password was requested but sshpass is absent.
    fn require_sshpass() -> Result<()> {
        match Command::new("sshpass").arg("-V").output() {
            Ok(_) => Ok(()),
            Err(_) => bail!(
                "password auth requires `sshpass`, which is not installed; \
                 install it or clone with an empty --password to stay prompt-free"
            ),
        }
    }
}
