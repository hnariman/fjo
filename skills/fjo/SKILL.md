---
name: fjo
description: Create passwordless ssh keys and clone git repositories without ever
  prompting. Use when the user wants a new ssh key generated, a repo cloned with
  ssh auth, or a preflight check of git/ssh tooling. Wraps the fjo rust CLI,
  which fails fast with clear errors instead of asking questions.
---

# fjo — ssh keys and git cloning for agents

`fjo` is a non-interactive git/ssh helper. Every command completes or fails cleanly.
**Never expects stdin; never hangs waiting for a human.**

If that fails, install it — both paths end on `PATH` as `fjo`:

Fedora / Alma / Rocky (dnf repo hosted on Forgejo):

```sh
# fedora:
dnf config-manager addrepo --from-repofile=https://git.home/api/packages/hnariman/rpm.repo
# rocky / alma 9:
dnf config-manager addrepo --from-repofile=https://git.home/api/packages/hnariman/rpm/el9.repo
dnf install fjo
```

Any host with rust/cargo:

```sh
cargo install --git https://git.home/hnariman/fjo.git
```

Requires either the dnf repo (no rust needed) or rust on the host. Verify with
`fjo --version` afterwards.</think><tool_call>edit<arg_key>newString</arg_key><arg_value>## Bootstrap (only once, only if missing)

Check whether the binary exists:

```sh
fjo --version
```

If it's missing, install it — same result on different hosts:

**Fedora / Alma / Rocky** (dnf repo hosted on Forgejo):

```sh
# fedora:
dnf config-manager addrepo --from-repofile=https://git.home/api/packages/hnariman/rpm.repo
# rocky / alma 9:
dnf config-manager addrepo --from-repofile=https://git.home/api/packages/hnariman/rpm/el9.repo
dnf install fjo
```

**Any host with rust/cargo:**

```sh
cargo install --git https://git.home/hnariman/fjo.git
```

Verify with `fjo --version` afterwards. If the RPM was installed, this skill file
already exists on disk (`/usr/share/fjo/skills/fjo/SKILL.md`) and nothing else is needed.

## Commands

All commands live in the **current working directory's context**: keys are stored in
and looked up under `{cwd}/.ssh/`. Run them from the project you care about.

### Preflight (always safe, never side-effecting)

```sh
fjo status
```

Reports the key directory, found keys, whether a key is resolvable for clone, and
git/ssh-keygen/sshpass availability. Run this first to decide between `setup` and `clone`.

### Generate a key

```sh
fjo setup --name <key-name> [--force]
```

- `--name` is **required**; there is no prompt fallback
- writes `{cwd}/.ssh/<key-name>` and edits nothing else
- outputs the public key — pass it to the remote (e.g. GitHub deploy keys) before cloning
- refuses to overwrite without `--force`

### Clone a repository

```sh
fjo clone <repo-url> [<dest>] [--key <name>] [--password <pw>]
```

- key auto-detection: explicit `--key` > a file named `key` > the single other key
  in `{cwd}/.ssh` > error telling you to pass `--key`
- **never prompts**: ssh runs with `BatchMode=yes` by default, so a missing or
  unregistered key fails immediately (usually `Permission denied (publickey)`)
  rather than hanging. If you see that error, the key exists but isn't registered
  on the remote — don't retry blindly; surface it
- `--password` is opt-in (needs `sshpass` on PATH); see the security note below

## Exit codes and JSON

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | operation failed — error text on stderr |
| 2 | usage error (bad arguments) |

For machine-readable output add the `--json` flag anywhere:

```sh
fjo --json setup --name deploy-key
# {"ok": true, "message": "...", "data": {"key_path": "...", "public_key": "ssh-ed25519 ..."}}

fjo --json clone git@github.com:user/repo.git
# {"ok": true, "message": "cloned ...", "data": {"key_path": "..."}}
# {"ok": false, "error": "..."} on failure, exit 1
```

Parse `data.public_key` to register the key on a remote; parse `data.key_path` to
know which key was chosen.

## Typical flow

```sh
fjo status                              # 1. preflight
fjo setup --name deploy-key             # 2. key (skip if key_resolvable is already true)
#    → copy printed public key to the remote host (deploy keys / SSH keys settings)
fjo clone git@github.com:user/repo.git  # 3. clone; use --key <name> if several keys exist
```

## Security notes

- Keys are passwordless ed25519 by design — treat `{cwd}/.ssh/` as sensitive and
  never print or exfiltrate the private half; `fjo` output only ever exposes the `.pub`
- `--password` values appear in process arguments (`ps`); avoid on shared hosts
- Ambiguous key sets are refused on purpose (`--key <name>` resolves them explicitly)
