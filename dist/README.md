# Distribution manifests

This directory holds packaging manifests for downstream distribution
channels. Each file is the canonical source for one channel and is
auto-bumped on every tag by a corresponding GitHub Action (where set
up). When adding a new channel, drop the manifest here and wire the
auto-bump in `.github/workflows/release.yml`.

## Channels

| Channel | File | Target repo | Auto-bump action |
| --- | --- | --- | --- |
| Homebrew | `homebrew/holdon.rb` | `imjustprism/homebrew-holdon` | `dawidd6/action-homebrew-bump-formula` |
| Scoop | `scoop/holdon.json` | `imjustprism/scoop-holdon` | inline shell step in `release.yml` |
| Winget | _generated at release time_ | `microsoft/winget-pkgs` (PR) | `vedantmgoyal2009/winget-releaser` |

## End-user install matrix

```sh
# crates.io
cargo install holdon

# binstall (downloads prebuilt)
cargo binstall holdon

# Homebrew (macOS, Linux)
brew install imjustprism/holdon/holdon

# Scoop (Windows)
scoop bucket add holdon https://github.com/imjustprism/scoop-holdon
scoop install holdon

# Winget (Windows)
winget install imjustprism.holdon

# Docker
docker pull ghcr.io/imjustprism/holdon

# Install script (any POSIX shell)
curl -fsSL https://raw.githubusercontent.com/imjustprism/holdon/main/install.sh | sh
```
