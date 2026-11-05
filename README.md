# fjo

**`fjo`** is a small, machine-friendly git/ssh helper built as a [Hermes](./hermes.json)-compatible CLI tool.
It creates passwordless ssh keys and clones git repositories with them — never prompting,
never hanging, and failing fast with a clear message when something can't work.

Designed for agents first: no stdin reads, one JSON object per run, documented exit codes,
and a manifest that describes everything the CLI can do.

---

## Features

- **Passwordless key generation** — `ssh-keygen` under the hood, ed25519 keys with no
  passphrase, written to `{cwd}/.ssh/<key_name>`
- **Prompt-free cloning** — `git clone` via `GIT_SSH_COMMAND`, pointing at the key in
  `{cwd}/.ssh/`. Keys are auto-detected: an explicit `--key` wins, then a key named
  `key`, then the only other candidate (refuses on ambiguity)
- **`--password` when needed** — by default ssh runs in `BatchMode=yes`, so it *cannot*
  prompt; when a password is actually required, `--password` feeds it non-interactively
  through `sshpass` (fails upfront with a clear error if sshpass is missing)
- **Preflight `status`** — one call reports the key directory, existing keys, whether a
  key is resolvable for clone, and git/ssh-keygen/sshpass availability
- **`--json` everywhere** — every command emits the same shape on stdout:
  `{"ok": bool, "message": str, "data": {...}}` on success,
  `{"ok": false, "error": str}` on failure. Agents parse one format, always
- **Tiny binary** — release builds are size-optimized (`opt-level=z`, LTO, stripped,
  ~570 KiB)

## Install

### Install with dnf (fedora / alma / rocky)

Forgejo's RPM package registry serves the repo directly — add it once, then
`dnf install` like any other package:

```sh
# fedora builds:
sudo dnf config-manager addrepo --from-repofile=https://git.home/api/packages/hnariman/rpm.repo
# rocky / alma 9 builds (separate glibc-compatible artifacts):
sudo dnf config-manager addrepo --from-repofile=https://git.home/api/packages/hnariman/rpm/el9.repo

sudo dnf install fjo
```

The generated `.repo` file sets `gpgcheck=1` and points `gpgkey=` at the signing
key xtask publishes (`fjo-release`, exported from `~/.local/share/fjo/gnupg`).
dnf prompts to import the key on first install — accept it once and the repo is
trusted thereafter. Fine on a private/self-hosted forge; review if you point this
flow at a public URL.

The RPM contains the size-optimized binary plus the agent skill:

| Path | What |
|---|---|
| `/usr/bin/fjo` | the stripped release binary |
| `/usr/share/fjo/skills/fjo/SKILL.md` | agent skill file (agents copy or point at it, no repo checkout needed) |
| `/usr/share/fjo/skills/fjo/scripts/install.sh` | cargo bootstrap for non-rpm hosts |

Because fjo links glibc dynamically, builds are produced **per distro** and land in
separate registry groups — do not cross-install: an `el9` rpm on Fedora under-performs
its capabilities and a `fc` rpm outright fails on Rocky 9. Releases are driven locally
via xtask (below): run it on each distro family or a matching container and pass
`--group el9` on the Rocky/Alma side.

### From source

```sh
git clone https://git.home/hnariman/fjo fjo
cd fjo
cargo build --release
```

The binary is `target/release/fjo`.

### As a system-wide tool

```sh
cargo install --path .
```

Then `fjo --version` works from anywhere.

### Register with Hermes

fjo ships as a portable [Agent Plugins v1](https://agent-plugins.org/specification)
package — no Python needed — so it installs with Hermes' plugin workflow:

```sh
hermes plugins install https://git.home/hnariman/fjo --no-enable   # clone from the git remote
hermes plugins enable fjo                                          # activate skills in sessions
```

The layout it reads:

```
plugin.json                      portable v1 manifest ($schema, name, version,…)
skills/fjo/SKILL.md              agent-facing instructions: commands, exit codes, JSON
skills/fjo/scripts/install.sh    bootstrap that cargo-builds and PATH-installs the binary
```

How the pieces connect: the **skill** teaches the agent the full command surface
(arguments, auto-detection precedence, exit codes, `--json` shapes) and when to use each
command; the **bootstrap script** covers hosts where `fjo` isn't on `PATH` yet
(requires rust/cargo); the **manifest** pins name, version and metadata for discovery.
Everything doubles as plain documentation for non-Hermes agent harnesses.

The manifest and skill are pinned by a test (`plugin_manifest_matches_code`): version,
the closed v1 schema fields, `SKILL.md` frontmatter and the bootstrap script are
validated against the code — bumping `Cargo.toml` without touching `plugin.json` fails CI.

## Usage

### Keys

```sh
# generate a key named "deploy-key" in {cwd}/.ssh
fjo setup --name deploy-key
#    key written: /home/you/project/.ssh/deploy-key
#    public key:
#    ssh-ed25519 AAAAC3… fjo-deploy-key

# refuse to overwrite; explicit override needed to clobber
fjo setup --name deploy-key             # errors: already exists
fjo setup --name deploy-key --force     # overwrites
```

### Clone

```sh
# auto-detects the key in {cwd}/.ssh (no --key needed for a single key)
fjo clone git@github.com:user/repo.git

# target directory and explicit key
fjo clone git@github.com:user/repo.git my-dir --key deploy-key

# password (rare; only when the remote needs auth beyond the key)
fjo clone git@github.com:user/repo.git --password 'hunter2'
```

The ssh command fjo assembles for you:

```sh
GIT_SSH_COMMAND="ssh -i {cwd}/.ssh/<key> -o IdentitiesOnly=yes" git clone <repo>
```

With an empty `--password` (the default), `BatchMode=yes` is appended so ssh can never
stop and ask for anything.

### Preflight

```sh
fjo status
# keys in /home/you/project/.ssh: [deploy-key]; key resolvable: true;
# git: true; ssh-keygen: true; sshpass: true
```

```sh
fjo --json status | jq '.data'
# {
#   "keys_dir": "/home/you/project/.ssh",
#   "keys": ["deploy-key"],
#   "key_resolvable": true,
#   "git_ok": true,
#   "ssh_keygen_ok": true,
#   "sshpass_ok": true
# }
```

### JSON output

Every command supports `--json`, globally:

```sh
fjo --json setup --name deploy-key
# {"ok": true, "message": "…public key…", "data": {"key_path": "…", "pub_path": "…", "public_key": "ssh-ed25519 …"}}

fjo --json clone git@github.com:user/repo.git
# {"ok": true, "message": "cloned git@github.com:…", "data": {"key_path": "…"}}

fjo --json clone git@github.com:bad/repo.git
# {"ok": false, "error": "git clone failed (128), …"}   (exit code 1)
```

## Command reference

| Command | Args | Description |
|---|---|---|
| `fjo setup --name <NAME> [--force]` | `--name` **required** | generate a passwordless ed25519 key in `{cwd}/.ssh/<NAME>`, print the public half |
| `fjo clone <REPO> [<DEST>] [--key <NAME>] [--password <PW>]` | `<REPO>` **required** | git clone with the key auto-detected under `{cwd}/.ssh`; never prompts |
| `fjo status` | — | preflight snapshot of keys and toolchain availability |
| `fjo --help` / `fjo <cmd> --help` | — | clap-generated help |

## Development

```sh
cargo test            # unit + manifest-sync tests
cargo build --release # size-optimized, stripped binary
```

Project layout:

```
src/main.rs               entrypoint: parse once, emit once (human text or JSON)
src/cli.rs                Cli struct: command surface (setup, clone, status), prompts strictly absent
src/key.rs                Key struct: key path resolution, auto-detection, ssh-keygen invocation
src/git.rs                Git struct: git operations via GIT_SSH_COMMAND
src/status.rs             preflight snapshot (keys, git, ssh-keygen, sshpass)
fjo.spec                  rpm packaging spec (prebuilt-binary tarball)
xtask                     release tooling: cargo xtask package | release | check
plugin.json               portable Agent Plugins v1 manifest (Hermes plugin discovery)
skills/fjo/SKILL.md       agent-facing skill: command reference, exit codes, bootstrap
skills/fjo/scripts/       install.sh — cargo build + PATH install bootstrap
```

Release flow (drives rpm packaging + forgejo publishing locally, no CI needed):

```sh
cargo xtask package                      # build binary + rpm into target/rpm/
cargo xtask release                      # package + publish (needs FORGEJO_TOKEN)
cargo xtask --no-build release           # publish an already-built rpm
cargo xtask --group el9 release          # rocky/alma builds -> el9 registry group
cargo xtask check                        # version sync only, no side effects
```

Published settings default to `https://git.home` / owner `hnariman`; override with
`--forgejo-url` / `--owner` / `--group` or the matching `FORGEJO_*` env vars. The
release is tagged by the version (`v`-less): make sure it matches the version git tag
you pushed.

## Design notes

- **No prompts, ever** — there is no place in the codebase that reads stdin; the agent
  gets a deterministic non-zero exit instead of a question
- **Key auto-detection precedence** — explicit `--key` > a file named `key` > the single
  other candidate > error mentioning `--key`
- **Fail loud, fail early** — bad input exits before doing anything; instrument
  availability (like `sshpass`) is checked *before* git runs, not after
- **Security note** — `--password` values appear in process arguments (`ps`/`/proc`);
  fine for local/trusted use, avoid on shared hosts

## License

See [LICENSE](./LICENSE) if present.
