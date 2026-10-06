# waytime

![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)
![MIT](https://img.shields.io/badge/license-MIT-green)
![platform](https://img.shields.io/badge/platform-Wayland%20%7C%20KDE-blue)

Zero-overhead screen time tracker for Wayland & KDE Plasma written in Rust.

## Features

- **Zero-overhead:** Event-driven architecture via KWin scripts and DBus signals (no busy loops, ~1.5MB RAM).
- **Accurate tracking:** Automatically pauses on screen lock (`Meta+L`), display sleep, and system suspend.
- **CLI Reporting:** Daily, weekly, monthly summaries with optional hierarchical breakdown (`-d`).
- **Persistent storage:** Fast, crash-resilient SQLite database with WAL mode and idempotent sessions.
- **Configurable filters:** Ignore specific applications via `~/.config/waytime/config.toml`.
- **Status bar ready:** Machine-readable `--json` output for Waybar, Polybar, or custom scripts.

## Installation (Arch Linux)

### Via Cargo (crates.io)
```bash
cargo install waytime
```

### Pre-built Binary (GitHub Releases)
Download the latest archive from [Releases](https://github.com/nxoji/waytime/releases/latest):
```bash
tar -xzf waytime-*-linux-x86_64.tar.gz
cd waytime-*-linux-x86_64
install -Dm755 waytime ~/.local/bin/waytime
install -Dm644 waytime.service ~/.config/systemd/user/waytime.service
```

### Arch Linux (PKGBUILD)
```bash
git clone https://github.com/nxoji/waytime.git
cd waytime
makepkg -si
```

### Starting the Daemon
Enable and start the background service:
```bash
systemctl --user enable --now waytime.service
```

Foreground mode (for testing or debugging):
```bash
waytime daemon
```

## Configuration

Configuration is automatically generated on the first run at `~/.config/waytime/config.toml`.

To quickly open it in your editor:
```bash
$EDITOR $(waytime --config-path)
```

Example `config.toml`:
```toml
# List of application IDs to ignore from tracking
ignore_apps = [
    "kwin_wayland",
    "plasmashell",
    "org.kde.plasmashell",
]
```

## Usage
```bash
# Today's report (default)
waytime

# Reports for different ranges
waytime -y        # yesterday
waytime -w        # last 7 days
waytime -m        # last 30 days

# Detailed window title breakdown
waytime -d
waytime -w -d

# Machine-readable JSON output
waytime --json
waytime -d --json

# Print configuration file path
waytime --config-path
```

## License
waytime is open source software under the [MIT License](https://opensource.org/license/mit/).
