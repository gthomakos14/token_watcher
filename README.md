# Antigravity Token Watcher

A GNOME Shell extension and high-performance Rust core for monitoring Google Antigravity AI token quotas and Claude Code usage limits in the top panel. It shows whichever tool you prompted most recently.

![Antigravity Token Watcher](extension/icons/antigravity.svg)

---

## Features

- **Automatic Claude / Antigravity Switching**: The logo, top bar percentage, and popup follow whichever tool received a prompt last.
- **Blazingly Fast Rust Backend**: Built in Rust with local caching, so a normal poll from the extension (a cache hit) takes about 1.2 ms and 5 MiB of memory. See [Performance](#performance).
- **Dual Support (CLI-First + IDE SQLite Fallback)**:
  - **CLI-First**: Directly interrogates the `agy` CLI (`/usage` and `/model` print-mode commands) for real-time, non-consuming live quota and active model synchronization.
  - **SQLite Fallback**: Retains full zero-copy wire-format protobuf parsing of `state.vscdb` when using the Antigravity IDE.
- **Real-time Quota Display**: Displays current quota percentage for primary models (e.g. `⚡ 76%` or `Flash: 76% | Claude: 13%`).
- **Instant Reactive Updates**: Monitors `~/.gemini/antigravity-cli/history.jsonl` and `state.vscdb` via `Gio.FileMonitor` (`inotify`) to update the top bar as soon as a prompt or turn completes.
- **Detailed Popup Menu**:
  - User account name, email, subscription plan tier (`Starter Quota`), and active model badge.
  - Model quota pools (Gemini Flash & Pro pool, Claude & GPT pool).
  - Visual color-coded progress bars (Green > 50%, Amber 20–50%, Red < 20%).
  - Time remaining until weekly quota reset countdown.
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
├── Cargo.toml          # Rust package dependencies (rusqlite, base64, serde, chrono, chrono-tz)
├── src/                # Rust core engine
│   ├── main.rs         # CLI interface (--json, --summary, human report)
│   ├── recency.rs      # Picks the most recently prompted provider
│   ├── claude_source.rs # Claude usage via `claude -p /usage`
│   ├── cli_source.rs   # Antigravity usage via the agy CLI, report caching
│   ├── db.rs           # SQLite reader for state.vscdb (read-only mode)
│   ├── parser.rs       # Protobuf wire parser & quota extractor
│   ├── models.rs       # Data structures & JSON serialization
│   └── test_util.rs    # Test-only helpers (fake CLI scripts)
├── extension/          # GNOME Shell extension (GNOME 45 & 46 ESM)
│   ├── metadata.json   # Extension metadata (UUID: antigravity-token-watcher@gthomakos14.github.com)
│   ├── extension.js    # PanelMenu.Button, async subprocess, and menu UI
│   ├── format.js       # Pure display logic (panel text, colors), unit tested with Node
│   ├── prefs.js        # Adw.PreferencesWindow settings dialog
│   ├── stylesheet.css  # GNOME Shell panel and menu styles
│   ├── icons/          # Antigravity and Claude icons
│   └── schemas/        # GSettings schema & compiled definitions
├── extension-tests/    # Node tests for extension/format.js
├── Makefile            # Build, test, package, and installation automation
└── README.md
```

---

## Claude Support

On each run, `token-watcher` compares the newest prompt in `~/.claude/history.jsonl` with the newest prompt in `~/.gemini/antigravity-cli/history.jsonl` and the newest file in `~/.gemini/antigravity/conversations` (Antigravity IDE activity). The later one wins. Slash commands such as `/model` are not counted as prompts. If `claude` isn't installed, Antigravity is always shown.

Claude usage comes from `claude -p /usage --output-format json --no-session-persistence`, which uses no tokens and writes no transcript. Its session and weekly limits appear as cards in the popup, and the top bar shows the lowest remaining percentage, since every limit applies at once. Reports are cached separately in `~/.cache/antigravity-token-watcher/claude-cache.json`. A fresh fetch contacts Anthropic's servers and takes about 0.7 s; cache hits take about 1.1 ms.

Reset times are read in the time zone that `/usage` names (for example `(America/New_York)`) and shown in your system time zone. If no zone is named, or the name isn't recognized, the system time zone is used.

The `/usage` output is undocumented human-readable text. If its format changes, the last cached report is shown for up to 7 days and `token-watcher` then reports an error.

Use `--provider claude` or `--provider antigravity` to override the automatic choice on the command line.

---

## Performance

Measured with [hyperfine](https://github.com/sharkdp/hyperfine) on a release build: AMD Ryzen 5 7600, Linux 7.0, rustc 1.98.1, November 2026. Each figure is the wall time of one whole `token-watcher --json` process, from start to exit.

**Local paths** (no network; at least 200 runs each after 20 warmup runs):

| Scenario | Mean | Range | Peak memory |
|:---|---:|---:|---:|
| Cache hit, provider chosen automatically (what the extension runs) | 1.2 ms ± 0.1 | 1.0–4.0 ms | 5.1 MiB |
| Cache hit, `--provider claude` | 1.1 ms ± 0.1 | 0.9–4.7 ms | 5.1 MiB |
| Cache hit, `--provider antigravity` | 1.1 ms ± 0.1 | 0.9–1.6 ms | 5.2 MiB |
| SQLite read (`--db`) | 1.4 ms ± 0.1 | 1.2–1.9 ms | 6.7 MiB |

**Fresh fetches** (`--refresh`; these call the CLIs over the network, so 5 runs each):

| Scenario | Mean | Range | Peak memory |
|:---|---:|---:|---:|
| Claude (`claude -p /usage`) | 663 ms ± 22 | 653–703 ms | 263 MiB |
| Antigravity (`agy` `/usage` and `/model`) | 2.8 s ± 2.0 | 1.7–6.4 s | 201 MiB |

Fresh fetches are dominated by the `claude` and `agy` CLIs, including their startup and network round trips, so they depend on your connection and on those services. Peak memory for a fresh fetch is the CLI's, not `token-watcher`'s. Fetches happen at most once per cache TTL (60 s by default), or when you press Refresh; every other poll is a cache hit.

To reproduce, install hyperfine (`cargo install hyperfine`) and run `make bench`. That writes the tables to `bench/local.md` and `bench/network.md`. `make bench-local` skips the network fetches.

---

## How the Rust Backend Works

The SQLite path of the Rust core (`token-watcher`) makes no network requests and takes about 1.4 ms per run. It extracts quota details directly from the local Antigravity IDE storage. Fresh fetches through the `agy` and `claude` CLIs do go over the network (see [Performance](#performance)).

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
This installs the extension bundle into `~/.local/share/gnome-shell/extensions/antigravity-token-watcher@gthomakos14.github.com`.

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
