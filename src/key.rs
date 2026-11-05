use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// An ssh keypair under {cwd}/.ssh, addressed by its base name.
pub struct Key {
    /// Path to the private key.
    pub key: PathBuf,
    /// Path to the corresponding `.pub` file.
    pub pub_key: PathBuf,
}

/// {cwd}/.ssh - where fjo stores and looks up keys.
pub fn ssh_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".ssh")
}

impl Key {
    /// Build the paths for `name` and validate the name.
    pub fn new(name: &str) -> Result<Self> {
        let name = name.trim();
        if name.is_empty() {
            bail!("key name must not be empty");
        }
        if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
            bail!("invalid key name: {name:?}");
        }
        let dir = ssh_dir();
        Ok(Self {
            key: dir.join(name),
            pub_key: dir.join(format!("{name}.pub")),
        })
    }

    /// Base names of private-key-like files in {cwd}/.ssh, no particular
    /// order; `.pub` halves and `known_hosts` are excluded.
    pub fn list() -> Vec<String> {
        let dir = ssh_dir();
        let Ok(rd) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            // strip `.pub` from the name, the private half wins the listing
            .filter(|e| e.path().extension().map(|x| x != "pub").unwrap_or(true))
            .filter(|e| e.file_name().to_string_lossy() != "known_hosts")
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Resolve the key to use for `clone`: an explicit name wins, then a
    /// file called `key`, then the single other candidate in {cwd}/.ssh.
    /// Bails when nothing usable or ambiguous candidates are found.
    pub fn pick(name: Option<&str>) -> Result<Self> {
        let dir = ssh_dir();

        if let Some(name) = name {
            let key = Self::new(name)?;
            if !key.key.is_file() {
                bail!("key file not found: {}", key.key.display());
            }
            return Ok(key);
        }

        let default = dir.join("key");
        if default.is_file() {
            return Ok(Self::from_path(default));
        }

        let mut candidates: Vec<PathBuf> = match fs::read_dir(&dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension().map(|x| x != "pub").unwrap_or(true)
                        && p.file_name().map(|f| f != "known_hosts").unwrap_or(true)
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        match candidates.len() {
            0 => bail!(
                "no ssh key found in {} (run `fjo setup` first)",
                dir.display()
            ),
            1 => Ok(Self::from_path(
                candidates
                    .pop()
                    .expect("candidates.len() == 1 checked above"),
            )),
            _ => bail!(
                "multiple keys in {} and no default `key`: pass --key <name>",
                dir.display()
            ),
        }
    }

    /// Paths for an already-resolved private key file.
    fn from_path(key: PathBuf) -> Self {
        Self {
            pub_key: key.with_extension("pub"),
            key,
        }
    }

    /// Whether any part of the keypair is already on disk.
    pub fn exists(&self) -> bool {
        self.key.exists() || self.pub_key.exists()
    }

    /// Generate a passwordless ed25519 keypair at the key path.
    pub fn generate(&self, force: bool) -> Result<()> {
        if self.exists() && !force {
            bail!(
                "{} already exists (use --force to overwrite)",
                self.key.display()
            );
        }

        let dir = self
            .key
            .parent()
            .context("key path has no parent directory")?;
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;

        let key_path = self
            .key
            .to_str()
            .context("key path is not valid utf-8")?;

        let out = Command::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-N",
                "", // no passphrase
                "-f",
                key_path,
                "-C",
                &format!(
                    "fjo-{}",
                    self.key
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            ])
            .output()
            .context("failed to run ssh-keygen")?;

        if !out.status.success() {
            bail!(
                "ssh-keygen failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    /// The ssh command line that points git at this key.
    pub fn ssh_command(&self) -> String {
        format!("ssh -i {} -o IdentitiesOnly=yes", self.key.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_new_rejects_bad_names() {
        assert!(Key::new("").is_err());
        assert!(Key::new("  ").is_err());
        assert!(Key::new("a/b").is_err());
        assert!(Key::new("..").is_err());
        assert!(Key::new("good-name").is_ok());
    }
}
