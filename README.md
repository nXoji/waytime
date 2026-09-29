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

## Installation (Arch Linux)

```bash
git clone https://github.com/nxoji/waytime.git
cd waytime
makepkg -si
```
Enable and start the background daemon:
```bash
systemctl --user enable --now waytime.service
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
```

# License
waytime is open source software under the [MIT License](https://opensource.org/license/mit/).
