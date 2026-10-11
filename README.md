# waytime

[![crates.io](https://img.shields.io/crates/v/waytime.svg)](https://crates.io/crates/waytime)
![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)
![MIT](https://img.shields.io/badge/license-MIT-green)
![platform](https://img.shields.io/badge/platform-Wayland%20%7C%20KDE-blue)

Zero-overhead screen time tracker for Wayland & KDE Plasma written in Rust.

## Features

- **Zero-overhead:** Event-driven architecture via KWin scripts and D-Bus signals (no busy loops, ~1.5MB RAM).
- **Accurate tracking:** Automatically pauses on screen lock (`Meta+L`), display sleep, and system suspend.
- **Categorization & Limits:** Map applications into custom categories and enforce daily time limits.
- **Desktop Notifications:** Real-time freedesktop D-Bus alerts via `zbus` when daily screen time limits are reached.
- **Flexible Date Filtering:** Query specific dates, custom date intervals (`--from`/`--to`), or rolling windows (`--since`).
- **CLI Reporting:** Daily, weekly, monthly summaries, tree-like category breakdowns (`-C`), and window title details (`-d`).
- **Persistent storage:** Fast, crash-resilient SQLite database with WAL mode and transactional migrations.
- **Configurable filters:** Ignore system processes and lockscreens via `~/.config/waytime/config.toml`.
- **Status bar ready:** Machine-readable `--json` output for Waybar, Polybar, or custom scripts.

## Installation

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
# Applications to exclude from tracking
ignore_apps = [
    "kwin_wayland",
    "plasmashell",
    "org.kde.plasmashell",
    "kscreenlocker_greet",
    "swaylock",
    "hyprlock"
]

[categories]
# Map category names to lists of application IDs (case-insensitive)
Development = [
    "code",
    "com.microsoft.VSCode",
    "nvim",
    "rustrover",
    "org.kde.konsole",
    "alacritty"
]
Browsers = [
    "firefox",
    "chromium"
]
Productivity = [
    "md.obsidian.Obsidian",
    "notion",
    "ONLYOFFICE"
]
Communication = [
    "org.telegram.desktop",
    "discord"
]

[limits]
# Daily limits for categories or individual applications
# Supported units: d, h, m, s (e.g., "2h", "45m", "1h 30m")
Communication = "1h 30m"
Browsers = "3h"

# App-level limits override category limits
discord = "45m"
firefox = "2h"
```

## Usage
```bash
# Today's report (default)
waytime

# Reports for different ranges
waytime -y        # yesterday
waytime -w        # last 7 days
waytime -m        # last 30 days

# Group report by configured categories with tree view
waytime -C

# Combine category grouping with time periods
waytime -w -C

# Specific calendar date (YYYY-MM-DD, today, yesterday)
waytime --date 2026-10-10
waytime --date yesterday

# Arbitrary date interval
waytime --from 2026-10-01 --to 2026-10-10

# From a specific date until now
waytime --from 2026-10-05

# Rolling time duration window
waytime --since 3d
waytime --since 12h
waytime --since 45m

# Detailed window title breakdown
waytime -d
waytime -w -d

# Machine-readable JSON output
waytime --json
waytime -C --json
waytime -d --json

# Print configuration file path
waytime --config-path
```

## License
waytime is open source software under the [MIT License](https://opensource.org/license/mit/).
