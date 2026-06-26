# tmussh

`tmussh` is a small Rust CLI that combines `ssh` and remote `tmux` session selection.

```sh
tmussh user@host
```

The command lists tmux sessions on the remote host, opens a local terminal UI, and then attaches to the selected session over `ssh -t`. It can also create a new remote tmux session.

## Install

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/thisdotrob/tmussh/main/scripts/install.sh | sh
```

The installer downloads the latest GitHub release for your platform and installs `tmussh` to `~/.local/bin` by default. Set `TMUSSH_INSTALL_DIR` to choose another location.

## Requirements

- A local `ssh` binary.
- A reachable SSH destination such as `user@host`.
- `tmux` installed on the remote host.

`tmussh` delegates SSH transport to the system `ssh` command, so existing SSH config, authentication, host-key prompts, agents, and jump hosts continue to work as they normally do.

Remote tmux commands run through `/bin/sh` and discover `tmux` with `command -v` plus common install locations, including Homebrew, system paths, Snap, Nix, and user-local bin directories. `tmussh` runs remote tmux with `-u` so direct SSH commands still use UTF-8 output when the remote shell does not load an interactive locale.

For compatibility with hosts that do not have newer terminal definitions installed, `tmussh` exposes `TERM=xterm-256color` to remote tmux by default. Override it when the remote host has the terminfo entry you want to use:

```sh
tmussh --remote-term xterm-ghostty user@host
```

If tmux is installed somewhere unusual, pass the path explicitly:

```sh
tmussh --tmux-path /custom/path/to/tmux user@host
```

## Usage

```sh
cargo run -- user@host
```

When remote sessions exist:

- Use Up/Down or `j`/`k` to move.
- Press Enter to attach to the selected session.
- Press `n` to create a new session.
- Press `q` or Esc to quit.

When no remote tmux sessions exist, `tmussh` prompts for a new session name immediately.

New session names must use only letters, numbers, underscores, periods, and hyphens.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Releasing

Maintainers can publish a new minor or major release with:

```sh
scripts/release.sh minor
scripts/release.sh major
```

The script creates and merges a release PR, tags `main` as `vX.Y.Z`, and lets the release workflow publish the GitHub release assets. Set `TMUSSH_RELEASE_MERGE=0` to stop after opening the release PR.
