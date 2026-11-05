use std::process::Command;

use serde::Serialize;

use crate::key::{self, Key};

/// Environment snapshot agents check before calling setup/clone.
#[derive(Serialize)]
pub struct Status {
    /// Absolute path of the key dir fjo would use.
    pub keys_dir: String,
    /// Private key names found there.
    pub keys: Vec<String>,
    /// Whether a key would be picked for `clone` without --key.
    pub key_resolvable: bool,
    pub git_ok: bool,
    pub ssh_keygen_ok: bool,
    pub sshpass_ok: bool,
}

pub fn status() -> Status {
    let dir = key::ssh_dir();
    // never bails: a preflight reports the truth, even bad truth
    Status {
        keys_dir: dir.display().to_string(),
        keys: Key::list(),
        key_resolvable: Key::pick(None).is_ok(),
        git_ok: bin_ok("git"),
        ssh_keygen_ok: bin_ok("ssh-keygen"),
        sshpass_ok: bin_ok("sshpass"),
    }
}

/// True when the binary exists and runs at all.
fn bin_ok(bin: &str) -> bool {
    Command::new(bin).arg("--version").output().is_ok()
}
