# fjo RPM spec — builds from a prebuilt-binary source tarball.
# producing script (scripts/package.sh) stages:
#   fjo-<version>/fjo                     release binary for the build distro
#   fjo-<version>/skills/fjo/SKILL.md     agent skill file
#   fjo-<version>/skills/fjo/scripts/...  bootstrap installer
#
# glibc compatibility is preserved by building per distro (fedora:latest,
# rockylinux:9) and publishing per registry group; see .forgejo/workflows.

%global debug_package %{nil}

Name:           fjo
Version:        0.1.0
Release:        1%{?dist}
Summary:        Non-interactive git/ssh helper for agent workflows
License:        MIT
URL:            https://git.home/hnariman/fjo
Source0:        %{name}-%{version}.tar.gz

ExclusiveArch:  x86_64 aarch64

# the tool shells out to git, ssh and (rarely) sshpass
Requires:       git-core
Requires:       openssh-clients
Recommends:     sshpass

%description
fjo is a machine-friendly git/ssh helper built as a Hermes-compatible plugin
tool: ed25519 ssh keys without a passphrase, git cloning with key
auto-detection under {cwd}/.ssh, and a preflight status command. It never
prompts — every command either completes or fails fast with a clear message,
making it safe for agents.

This package ships the stripped, size-optimized release binary plus the
agent skill file under %{_datadir}/fjo/skills.

%prep
%autosetup -c

%install
mkdir -p %{buildroot}%{_bindir}
install -m 0755 fjo %{buildroot}%{_bindir}/fjo
mkdir -p %{buildroot}%{_datadir}/fjo/skills/fjo/scripts
install -m 0644 skills/fjo/SKILL.md \
    %{buildroot}%{_datadir}/fjo/skills/fjo/SKILL.md
install -m 0755 skills/fjo/scripts/install.sh \
    %{buildroot}%{_datadir}/fjo/skills/fjo/scripts/install.sh

%files
%{_bindir}/fjo
%{_datadir}/fjo/
%doc README.md

%changelog
* Thu Nov 05 2026 fjo packaging <fjo@localhost> - 0.1.0-1
- initial hermes-compatible CLI: setup, clone, status, --json
