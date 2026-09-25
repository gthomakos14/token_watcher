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
│   ├── metadata.json   # Extension metadata (UUID: antigravity-token-watcher@github.com/gthomakos14)
│   ├── extension.js    # PanelMenu.Button, async subprocess, and menu UI
│   ├── prefs.js        # Adw.PreferencesWindow settings dialog
│   ├── stylesheet.css  # GNOME Shell panel and menu styles
│   ├── icons/          # Antigravity icon
│   └── schemas/        # GSettings schema & compiled definitions
├── Makefile            # Build, test, package, and installation automation
└── README.md
```

---

## How the Rust Backend Works

The Rust core (`token-watcher`) is engineered for sub-2 millisecond execution, zero-overhead local polling, and zero external network requests. It extracts quota details directly from the local Antigravity IDE storage.

### 1. Read-Only SQLite State Access ([`src/db.rs`](file:///home/glenn/personal_projects/token_watcher/src/db.rs))
- **Path Resolution**: Automatically targets Antigravity's global state store at `~/.config/Antigravity/User/globalStorage/state.vscdb` (or a custom path via `--db`).
- **Non-blocking Concurrency**: The database is opened using SQLite URI read-only mode (`file:<path>?mode=ro`) with `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI`. This prevents database locks or collisions while the Antigravity IDE is actively writing tokens and chat histories to the database.
- **State Query**: Executes a targeted query against `ItemTable` for two synchronization keys:
  - `antigravityUnifiedStateSync.userStatus`: Houses serialized user account info, subscription tier, and real-time model quota structures.
  - `antigravityUnifiedStateSync.modelPreferences`: Houses agent model preferences and active model state.

### 2. Schema-less Protobuf Wire Parser ([`src/parser.rs`](file:///home/glenn/personal_projects/token_watcher/src/parser.rs))
Instead of relying on compiled `.proto` definitions or `protoc` build-time dependencies (which break whenever upstream schemas evolve), the backend implements a zero-copy, wire-format Protocol Buffers decoder:
- **Wire Type Support**: Decodes LEB128 varints (Type 0), 64-bit fixed words (Type 1), length-delimited byte slices (Type 2), and 32-bit IEEE 754 floats (Type 5).
- **Nested Base64 Unwrapping**: Antigravity wraps the state protobuf inside an outer Base64 payload, which indexes into a secondary length-delimited protobuf containing an inner Base64-encoded protobuf payload (`Base64 -> Protobuf Field 1.2 -> Base64 -> Inner Protobuf`).
- **Field Extraction**:
  - **User & Plan**: Extracts account display name (field 3), email (field 7), profile picture URL (field 38), and subscription plan tier (field 36).
  - **Quota List (Field 33)**: Iterates repeated model records (subfield 1). For each model, it decodes:
    - Model name (subfield 1, string) and model ID (subfield 2 -> 1, varint).
    - Status badge (subfield 16, e.g. `"Fast"`).
    - Quota metrics (subfield 15): extracts quota remaining fraction (32-bit float, scaled to 0–100%) and Unix epoch reset timestamp (subfield 2 -> 1, varint).
- **Relative Time Calculation**: Computes remaining countdown strings (e.g. `1d 1h`, `4h 20m`) and localized date-time strings using `chrono`.

### 3. Active Model Detection
- Decodes `antigravityUnifiedStateSync.modelPreferences` looking for the sentinel key `last_selected_agent_model_sentinel_key`.
- Extracts and maps the active model ID to mark `is_selected: true` on the matching quota record (indicated with an asterisk `*` in CLI reports and prioritized in the UI).

### 4. Output Modes & Serialization ([`src/main.rs`](file:///home/glenn/personal_projects/token_watcher/src/main.rs), [`src/models.rs`](file:///home/glenn/personal_projects/token_watcher/src/models.rs))
- **`--json`**: Serializes `TokenReport` into a clean JSON object for seamless consumption by the GNOME Shell extension asynchronously via `Gio.Subprocess`.
- **`--summary`**: Emits a compact, single-line string (e.g. `Flash: 86% | Claude: 80%`) for status bars like tmux, Polybar, Waybar, or i3blocks.
- **Default (Terminal)**: Renders a full, formatted CLI dashboard complete with Unicode progress bars (`█` / `░`) and reset countdowns.

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
This installs the extension bundle into `~/.local/share/gnome-shell/extensions/antigravity-token-watcher@github.com/gthomakos14`.

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
