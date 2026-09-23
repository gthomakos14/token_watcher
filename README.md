# Antigravity Token Watcher

A GNOME Shell extension and high-performance Rust core for monitoring Google Antigravity AI token quotas and model limits in the top panel.

![Antigravity Token Watcher](extension/icons/antigravity.svg)

---

## Features

- **Blazingly Fast Rust Backend**: Built in Rust to parse Antigravity's local SQLite database (`state.vscdb`) and protobuf wire format in under **2 milliseconds**.
- **Real-time Quota Display**: Displays current quota percentage for primary models (e.g. `⚡ 86%` or `Flash: 86% | Claude: 80%`).
- **Instant Reactive Updates**: Monitors `state.vscdb` via `Gio.FileMonitor` (`inotify`) to update the top bar as soon as Antigravity uses tokens or completes a prompt.
- **Detailed Popup Menu**:
  - User account name, email, and subscription plan tier (`Starter Quota`, etc.).
  - Per-model quota breakdown (Gemini Flash, Gemini Pro, Claude Sonnet, Claude Opus, GPT-OSS).
  - Visual color-coded progress bars (Green > 50%, Amber 20–50%, Red < 20%).
  - Time remaining until quota reset countdown.
  - "Refresh" and "Open Antigravity IDE" quick action buttons.
- **Configurable Preferences**:
  - Display format: Percentage only, Model + Percentage, Dual Flash/Claude view, or Icon only.
  - Prioritized model (Flash, Pro, Sonnet).
  - Panel box placement (Right, Center, Left).
  - Low quota warning threshold.
  - Custom refresh interval.
- **Standalone CLI**: Can also be used directly from terminal or scripts via `token-watcher`.

---

## Architecture

```
token_watcher/
├── Cargo.toml          # Rust package dependencies (rusqlite, base64, serde, chrono)
├── src/                # Rust core engine
│   ├── main.rs         # CLI interface (--json, --summary, human report)
│   ├── db.rs           # SQLite reader for state.vscdb (read-only mode)
│   ├── parser.rs       # Protobuf wire parser & quota extractor
│   └── models.rs       # Data structures & JSON serialization
├── extension/          # GNOME Shell extension (GNOME 45 & 46 ESM)
│   ├── metadata.json   # Extension metadata (UUID: antigravity-token-watcher@glenn.local)
│   ├── extension.js    # PanelMenu.Button, async subprocess, and menu UI
│   ├── prefs.js        # Adw.PreferencesWindow settings dialog
│   ├── stylesheet.css  # GNOME Shell panel and menu styles
│   ├── icons/          # Antigravity icon
│   └── schemas/        # GSettings schema & compiled definitions
├── Makefile            # Build, test, package, and installation automation
└── README.md
```

---

## Installation & Setup

### 1. Build the Rust binary and schemas
```bash
make build
```
This compiles the optimized Rust binary `target/release/token-watcher` and copies it into `extension/bin/token-watcher`, then compiles the GSettings schemas.

### 2. Install the GNOME Extension
```bash
make install
```
This installs the extension bundle into `~/.local/share/gnome-shell/extensions/antigravity-token-watcher@glenn.local`.

### 3. Activating in GNOME Shell
Because your desktop runs **GNOME Shell 46 on Wayland**, GNOME Shell loads newly added extension files into its runtime upon session login:
1. **Log out of your desktop session and log back in** (or reboot).
2. Enable the extension:
   ```bash
   make enable
   ```
   Or toggle it via the **Extensions** application (`gnome-extensions-app`).

---

## Standalone CLI Usage

You can also use the compiled Rust binary directly from your terminal or in custom scripts:

```bash
# Formatted human-readable report with progress bars
./target/release/token-watcher

# Single-line summary (ideal for polybar, tmux, or custom status bars)
./target/release/token-watcher --summary

# JSON output
./target/release/token-watcher --json
```

Example terminal output:
```
==========================================================
             ANTIGRAVITY TOKEN & QUOTA WATCHER            
==========================================================
User:  Hambone Fakenamington <Secretariatrulez96@hotmail.com>
Plan:  Antigravity Starter Quota
----------------------------------------------------------
MODEL                            REMAIN       RESETS IN  STATUS    
----------------------------------------------------------
 Gemini 3.6 Flash (High)        85.8% [█████████░]      1d 1h  Fast    
 Gemini 3.6 Flash (Medium)      85.8% [█████████░]      1d 1h  Fast    
 Gemini 3.6 Flash (Low)         85.8% [█████████░]      1d 1h  Fast    
 Gemini 3.1 Pro (High)          85.8% [█████████░]      1d 1h          
 Gemini 3.1 Pro (Low)           85.8% [█████████░]      1d 1h          
 Claude Sonnet 4.6 (Thinking)   79.5% [████████░░]      1d 1h          
 Claude Opus 4.6 (Thinking)     79.5% [████████░░]      1d 1h          
 GPT-OSS 120B (Medium)          79.5% [████████░░]      1d 1h          
==========================================================
```
