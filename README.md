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

### From source

```sh
git clone <this-repo> fjo
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

Point your Hermes manifest loader at [`hermes.json`](./hermes.json) — or copy it next to
your other tool manifests. It declares:

```json
{
  "name": "fjo",
  "version": "0.1.0",
  "bin": "target/release/fjo",
  "non_interactive": true,
  "exit_codes": { "0": "success", "1": "operation failed", "2": "usage error" }
}
```

| Key | Meaning |
|---|---|
| `bin` | executable Hermes should invoke |
| `non_interactive` | the tool never prompts; failures are errors, not questions |
| `exit_codes` | `0` success · `1` operation failed (stderr text or `--json` error object) · `2` argument-parsing error |
| `globals` | `--json`, valid before or after the subcommand |
| `commands` | `setup`, `clone`, `status` with full arg schemas |

The manifest is pinned by a test (`hermes_manifest_matches_code`), so version and command
names can't drift from the code — if you bump `Cargo.toml`, update `hermes.json` in the
same commit or CI fails.

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
src/main.rs   entrypoint: parse once, emit once (human text or JSON)
src/cli.rs    Cli struct: command surface (setup, clone, status), prompts strictly absent
src/key.rs    Key struct: key path resolution, auto-detection, ssh-keygen invocation
src/git.rs    Git struct: git operations via GIT_SSH_COMMAND
src/status.rs preflight snapshot (keys, git, ssh-keygen, sshpass)
hermes.json   the manifest that consumers (Hermes) read
```

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
