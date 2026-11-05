//! fjo release tooling - local replacement for CI runners.
//!
//! `cargo xtask package`  build the release binary and produce the rpm
//! `cargo xtask release`  package, then publish to the forgejo registry +
//!                        attach the artifacts to the git tag's release page
//! `cargo xtask check`    only version-consistency checks (no side effects)
//!
//! External tools used (all standard on rpm distros): tar, rpmbuild, curl.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Xtask {
    #[command(subcommand)]
    cmd: Cmd,
    /// Forgejo base url
    #[arg(long, default_value = "https://git.home", env = "FORGEJO_URL")]
    forgejo_url: String,
    /// Registry owner (identifies both the package namespace and the token)
    #[arg(long, default_value = "hnariman", env = "FORGEJO_OWNER")]
    owner: String,
    /// Registry group for the rpm ('' = default, 'el9' = rocky/alma builds)
    #[arg(long, default_value = "", env = "FORGEJO_GROUP")]
    group: String,
    /// Forgejo access token (publish only; env FORGEJO_TOKEN)
    #[arg(long, env = "FORGEJO_TOKEN")]
    token: Option<String>,
    /// Skip the cargo release build (reuse target/release/fjo as-is)
    #[arg(long, default_value_t = false, global = true)]
    no_build: bool,
    /// Skip rpm GPG signing (rpmsign + a local signing key)
    #[arg(long, default_value_t = false, global = true)]
    no_sign: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build + package the rpm into target/rpm/
    Package,
    /// Package and publish to forgejo (registry group + release page)
    Release,
    /// Version-consistency checks only, no side effects
    Check,
}

fn main() {
    let xtask = Xtask::parse();
    if let Err(err) = run(&xtask) {
        eprintln!("xtask: {err}");
        std::process::exit(1);
    }
}

fn run(xtask: &Xtask) -> Result<()> {
    match xtask.cmd {
        Cmd::Check => {
            check_versions()?;
        }
        Cmd::Package => {
            if !xtask.no_build {
                build_release()?;
            }
            let rpm = package()?;
            if !xtask.no_sign {
                sign_rpm(&rpm)?;
            }
            println!("rpm: {}", rpm.display());
        }
        Cmd::Release => {
            let token = xtask
                .token
                .clone()
                .context("publish needs a token: --token or FORGEJO_TOKEN")?;
            if !xtask.no_build {
                build_release()?;
            }
            let version = check_versions()?;
            let rpm = package()?;
            if !xtask.no_sign {
                sign_rpm(&rpm)?;
            }
            publish(xtask, &token, &version, &[rpm.as_path()])?;
            print_urls(xtask, &version);
        }
    }
    Ok(())
}

/// Workspace root: xtask sits one level under it.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask always has a parent")
        .to_path_buf()
}

fn ok(msg: &str) {
    println!("==> {msg}");
}

/// Fail loudly when Cargo.toml and fjo.spec drift apart; returns the version.
fn check_versions() -> Result<String> {
    let toml = fs::read_to_string(root().join("Cargo.toml")).context("read Cargo.toml")?;
    let cargo_version = toml
        .lines()
        .skip_while(|l| l.trim() != "[package]")
        .skip(1)
        .find_map(|l| {
            l.trim()
                .strip_prefix("version = ")
                .map(|v| v.trim_matches(|c| c == '"' || c == ' ').to_string())
        })
        .filter(|v| !v.is_empty())
        .context("no version = \"...\" found under [package] in Cargo.toml")?;

    let spec = fs::read_to_string(root().join("fjo.spec")).context("read fjo.spec")?;
    let spec_version = spec
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("Version:")
                .map(|v| v.trim().to_string())
        })
        .filter(|v| !v.is_empty())
        .context("no Version: line found in fjo.spec")?;

    if cargo_version != spec_version {
        bail!(
            "version drift: Cargo.toml={cargo_version} fjo.spec={spec_version}; sync one of them before releasing"
        );
    }
    ok(&format!("version {cargo_version} (Cargo.toml == fjo.spec)"));
    Ok(cargo_version)
}

/// cargo build --release in the workspace root, checked.
fn build_release() -> Result<()> {
    ok("cargo build --release");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .status()
        .context("running cargo")?;
    if !status.success() {
        bail!("cargo build --release failed");
    }
    Ok(())
}

/// Stage the prebuilt-binary tarball the spec expects, run rpmbuild, and put
/// the produced rpm into {root}/target/rpm. Returns its path.
fn package() -> Result<PathBuf> {
    let version = check_versions()?;
    let root = root();
    let dist = dist_tag()?;
    let arch = arch()?;
    // release "1" joined with the dist tag: fjo-1.fc44.x86_64.rpm; a missing
    // %dist (plain host or --undefine) collapses to just the release number
    let rpm_file = if dist.is_empty() {
        format!("fjo-{version}-1.{arch}.rpm")
    } else {
        format!("fjo-{version}-1.{dist}.{arch}.rpm")
    };

    let bin = root.join("target/release/fjo");
    if !bin.is_file() {
        bail!(
            "no release binary at {} (run cargo build --release or drop --no-build)",
            bin.display()
        );
    }

    let tmp = std::env::temp_dir().join(format!("fjo-xtask-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);

    // {tmp}/stage: tarball content. No top-level prefix: %autosetup -c in the
    // spec creates fjo-<version>/ itself.
    let stage = tmp.join("stage");
    copy_into(&root.join("skills"), &stage.join("skills")).context("staging skills")?;
    copy_into(&root.join("README.md"), &stage.join("README.md")).context("staging README")?;
    copy_into(&bin, &stage.join("fjo")).context("staging binary")?;
    set_exec(&stage.join("fjo"))?;

    // rpmbuild tree
    let rpmd = tmp.join("rpmbuild");
    for dir in ["BUILD", "RPMS", "SOURCES", "SPECS"] {
        fs::create_dir_all(rpmd.join(dir))?;
    }
    fs::copy(root.join("fjo.spec"), rpmd.join("SPECS/fjo.spec"))?;

    ok("creating source tarball");
    sh(
        &stage,
        "tar",
        &[
            "-czf".into(),
            rpmd.join("SOURCES")
                .join(format!("fjo-{version}.tar.gz"))
                .into_os_string()
                .into_string()
                .expect("utf8 path"),
            ".".into(),
        ],
    )
    .context("tar (is it installed?)")?;

    ok("rpmbuild -bb");
    sh(
        &rpmd,
        "rpmbuild",
        &[
            "-bb".into(),
            "--define".into(),
            format!("_topdir {}", rpmd.display()),
            "--define".into(),
            format!("_sourcedir {}", rpmd.join("SOURCES").display()),
            "SPECS/fjo.spec".into(),
        ],
    )
    .context("rpmbuild (dnf install rpm-build)")?;

    let built = rpmd.join("RPMS").join(&arch).join(&rpm_file);
    if !built.is_file() {
        bail!(
            "rpmbuild ran but the expected rpm is missing: {}",
            built.display()
        );
    }

    let out_dir = root.join("target/rpm");
    fs::create_dir_all(&out_dir)?;
    let out = out_dir.join(built.file_name().unwrap());
    fs::copy(&built, &out)?;
    let _ = fs::remove_dir_all(&tmp);

    ok(&format!("rpm -> {}", out.display()));
    Ok(out)
}

/// PUT the rpm into the registry group; create or reuse the release for the
/// version tag; attach the artifacts (201 ok, 409 idempotent).
fn publish(xtask: &Xtask, token: &str, version: &str, artifacts: &[&Path]) -> Result<()> {
    let rpm = artifacts
        .first()
        .copied()
        .context("publish needs at least the rpm artifact")?;
    let file = rpm
        .file_name()
        .and_then(|f| f.to_str())
        .context("non-utf8 rpm filename")?
        .to_string();

    let auth = format!("{}:{token}", xtask.owner);
    let base = xtask.forgejo_url.trim_end_matches('/');
    let registry_base = if xtask.group.is_empty() {
        format!("{base}/api/packages/{}/rpm", xtask.owner)
    } else {
        format!("{base}/api/packages/{}/rpm/{}", xtask.owner, xtask.group)
    };
    let repo_api = format!("{base}/api/v1/repos/{}/{}/releases", xtask.owner, "fjo");

    ok(&format!(
        "publishing {file} to registry (group '{}')",
        if xtask.group.is_empty() {
            "<default>"
        } else {
            &xtask.group
        }
    ));
    let code = curl_upload(&auth, rpm, &format!("{registry_base}/upload"))?;
    match code.as_str() {
        "201" => ok("registry: created"),
        "409" => ok("registry: same file already there, keeping it"),
        _ => {
            bail!("registry upload -> HTTP {code} (401 suggests token/scopes; 400 a malformed rpm)")
        }
    }

    // Publish/refresh the armored public key as a generic package so dnf
    // can verify signatures via gpgkey=. Same-file uploads return 409.
    let key_path = std::env::temp_dir().join("fjo-release-key.asc");
    export_pubkey(&key_path)?;
    let key_url = format!(
        "{base}/api/packages/{}/generic/fjo-release-key/key/fjo.asc",
        xtask.owner
    );
    let key_code = publish_pubkey(&auth, &key_path, &key_url);
    match key_code {
        Ok(()) => ok("registry: public key published"),
        Err(e) => bail!("{e}"),
    }

    let release_id = match get_json(&auth, &format!("{repo_api}/tags/{version}"))? {
        Some(json) => {
            let id = first_int(&json).context("existing release json has no id")?;
            ok(&format!("release {id} already exists for tag {version}"));
            id
        }
        None => {
            let payload = format!(
                "{{\"tag_name\":\"{version}\",\"name\":\"fjo {version}\",\"draft\":false,\"prerelease\":false}}"
            );
            let json = post_json(&auth, &repo_api, &payload)?;
            let id = first_int(&json).context("release create json has no id")?;
            ok(&format!("release {id} created for tag {version}"));
            id
        }
    };

    for artifact in artifacts {
        let name = artifact
            .file_name()
            .and_then(|f| f.to_str())
            .context("non-utf8 artifact name")?;
        ok(&format!("attaching {name} to release {version}"));
        let url = format!("{repo_api}/{release_id}/assets?name={name}");
        let code = curl_form(&auth, artifact, &url)?;
        match code.as_str() {
            "200" | "201" => {}
            other => bail!("asset upload -> HTTP {other}"),
        }
    }
    Ok(())
}

fn print_urls(xtask: &Xtask, version: &str) {
    let base = xtask.forgejo_url.trim_end_matches('/');
    let group_part = if xtask.group.is_empty() {
        String::new()
    } else {
        format!("/{}", xtask.group)
    };
    println!();
    println!(
        "    dnf repo: {base}/api/packages/{}/rpm{group_part}.repo",
        xtask.owner
    );
    println!(
        "    gpg key:  {base}/api/packages/{}/generic/fjo-release-key/key/fjo.asc",
        xtask.owner
    );
    println!(
        "    release:  {base}/{owner}/fjo/releases/tag/{version}",
        owner = xtask.owner
    );
}

// --- gpg signing ----------------------------------------------------------------

/// GNUPGHOME holding the fjo release signing key.
fn gnupg_home() -> PathBuf {
    if let Ok(home) = std::env::var("FJO_GNUPGHOME") {
        return PathBuf::from(home);
    }
    if let Ok(home) = std::env::var("HOME") {
        let dir = Path::new(&home).join(".local/share/fjo/gnupg");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        return dir;
    }
    std::env::temp_dir().join("fjo-gnupg")
}

/// The uid xtask signs under.
const GPG_UID: &str = "fjo-release";

/// Make sure a signing key exists (unprotected, rsa3072, sign-only) in
/// GNUPGHOME; harmless to call repeatedly.
fn ensure_gpg_key() -> Result<()> {
    let home = gnupg_home();
    let out = Command::new("gpg")
        .env("GNUPGHOME", &home)
        .args(["--list-keys", "--with-colons", GPG_UID])
        .output()
        .context("running gpg (is it installed?)")?;
    if out.status.success() && String::from_utf8_lossy(&out.stdout).contains("sec:") {
        return Ok(());
    }
    ok("generating gpg signing key (fjo-release, no passphrase)");
    let batch = format!(
        "%no-protection\nKey-Type: RSA\nKey-Length: 3072\nKey-Usage: sign\n\
         Name-Real: {GPG_UID}\nName-Email: release@fjo.invalid\nExpire-Date: 0\n%commit\n"
    );
    let script = std::env::temp_dir().join(format!("fjo-gpg-{}.conf", std::process::id()));
    fs::write(&script, batch)?;
    let status = Command::new("gpg")
        .env("GNUPGHOME", &home)
        .args(["--batch", "--gen-key", "--quiet"])
        .arg(&script)
        .status()
        .context("running gpg")?;
    let _ = fs::remove_file(&script);
    if !status.success() {
        bail!("gpg --gen-key failed in {}", home.display());
    }
    Ok(())
}

/// Sign the rpm in place with the local release key (requires the `rpm-sign`
/// package for rpmsign). Idempotent: re-signing just refreshes the signature.
fn sign_rpm(rpm: &Path) -> Result<()> {
    if Command::new("rpmsign")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        bail!("rpmsign not found — `dnf install rpm-sign` (or rerun with --no-sign)");
    }
    ensure_gpg_key()?;
    let home = gnupg_home();
    ok("gpg-signing rpm (fjo-release)");
    sh(
        &root(),
        "rpmsign",
        &[
            "--addsign".into(),
            "--define".into(),
            format!("_gpg_name {GPG_UID}"),
            "--define".into(),
            format!("_gpg_path {}", home.display()),
            rpm.display().to_string(),
        ],
    )
    .context("rpmsign --addsign")?;
    Ok(())
}

/// Write the armored public key to `out`.
fn export_pubkey(out: &Path) -> Result<()> {
    ensure_gpg_key()?;
    let gpg_out = Command::new("gpg")
        .env("GNUPGHOME", gnupg_home())
        .args(["--armor", "--export", GPG_UID])
        .output()
        .context("running gpg")?;
    if !gpg_out.status.success() || gpg_out.stdout.is_empty() {
        bail!(
            "gpg --export failed: {}",
            String::from_utf8_lossy(&gpg_out.stderr)
        );
    }
    fs::write(out, &gpg_out.stdout)?;
    Ok(())
}

// --- subprocess helpers -------------------------------------------------------

/// Run `prog` with output shown; check it exited zero.
fn sh(dir: &Path, prog: &str, args: &[String]) -> Result<()> {
    let status = Command::new(prog)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("running {prog}"))?;
    if !status.success() {
        bail!("{prog} exited nonzero");
    }
    Ok(())
}

fn dist_tag() -> Result<String> {
    let out = Command::new("rpmbuild")
        .args(["--eval", "%{?dist}"])
        .output()
        .context("rpmbuild --eval (dnf install rpm-build?)")?;
    if !out.status.success() {
        bail!(
            "rpmbuild --eval failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let mut s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.starts_with('.') {
        s.remove(0);
    }
    Ok(s)
}

fn arch() -> Result<String> {
    let out = Command::new("uname")
        .args(["-m"])
        .output()
        .context("uname -m")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn curl_upload(auth: &str, file: &Path, url: &str) -> Result<String> {
    curl_code(&[
        "-sS".into(),
        format!("--user={auth}").into(),
        "-o".into(),
        "/dev/null".into(),
        "-w".into(),
        "%{http_code}".into(),
        format!("--upload-file={}", file.display()).into(),
        url.into(),
    ])
}

/// Generic-package key upload: DELETE first so a stale/corrupted key file
/// can never wedge the publish step (a plain PUT to an existing file 409s).
fn publish_pubkey(auth: &str, file: &Path, url: &str) -> Result<()> {
    let _ = curl_code(&[
        "-sS".into(),
        format!("--user={auth}").into(),
        "-o".into(),
        "/dev/null".into(),
        "-w".into(),
        "%{http_code}".into(),
        "-X".into(),
        "DELETE".into(),
        url.into(),
    ]);
    let code = curl_upload(auth, file, url)?;
    match code.as_str() {
        "201" => Ok(()),
        other => bail!("public key upload -> HTTP {other}"),
    }
}

fn curl_form(auth: &str, file: &Path, url: &str) -> Result<String> {
    curl_code(&[
        "-sS".into(),
        format!("--user={auth}").into(),
        "-o".into(),
        "/dev/null".into(),
        "-w".into(),
        "%{http_code}".into(),
        "-F".into(),
        format!("attachment=@{}", file.display()).into(),
        url.into(),
    ])
}

fn curl_code(args: &[std::ffi::OsString]) -> Result<String> {
    let out = Command::new("curl")
        .args(args)
        .output()
        .context("running curl")?;
    if !out.status.success() {
        bail!(
            "curl failed (network?): {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// GET json; Ok(None) for any non-200 (404 means "not yet").
fn get_json(auth: &str, url: &str) -> Result<Option<String>> {
    let out = Command::new("curl")
        .args(["-sS", "--user", auth, "-w", "\n%{http_code}", url])
        .output()
        .context("running curl")?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = split_status(&text)?;
    if code == "200" {
        Ok(Some(body.to_string()))
    } else {
        Ok(None)
    }
}

fn post_json(auth: &str, url: &str, body: &str) -> Result<String> {
    let out = Command::new("curl")
        .args([
            "-sS",
            "--user",
            auth,
            "-H",
            "Content-Type: application/json",
            "-w",
            "\n%{http_code}",
            "-d",
            body,
            url,
        ])
        .output()
        .context("running curl")?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (resp, code) = split_status(&text)?;
    if code != "201" {
        bail!("release create -> HTTP {code}: {}", resp.trim());
    }
    Ok(resp.to_string())
}

/// curl -w "\n%{http_code}" appends the status after a blank line.
fn split_status(text: &str) -> Result<(&str, String)> {
    match text.rfind('\n') {
        Some(i) => Ok((&text[..i], text[i + 1..].trim().to_string())),
        None => bail!("curl emitted no status suffix"),
    }
}

/// First integer that follows `"id"` in a forgejo json body.
fn first_int(json: &str) -> Option<u64> {
    let pos = json.find("\"id\"")?;
    let tail = &json[pos + "\"id\"".len()..];
    let digits: String = tail
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Copy a file or a directory tree into `dst` (dst may be new).
fn copy_into(src: &Path, dst: &Path) -> Result<()> {
    if src.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)?.filter_map(|e| e.ok()) {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_into(&path, &target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn set_exec(path: &Path) -> Result<()> {
    let mut perm = fs::metadata(path)?.permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_int_pulls_release_id() {
        assert_eq!(first_int(r#"{"id":42,"tag_name":"v1"}"#), Some(42));
        assert_eq!(first_int(r#"{"tag_name":"v1","id": 7 }"#), Some(7));
        assert_eq!(first_int(r#"{"tag_name":"v1"}"#), None);
        // nested "name" strings must not confuse the id extraction
        assert_eq!(
            first_int(r#"{"name":"fjo 1","id":12345,"other":"x"}"#),
            Some(12345)
        );
    }

    #[test]
    fn status_splitting() {
        assert_eq!(split_status("\n").unwrap(), ("", "".to_string()));
        assert_eq!(split_status("{}\n201").unwrap(), ("{}", "201".to_string()));
    }
}
