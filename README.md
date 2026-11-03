# Token Watcher for Antigravity & Claude Code

A GNOME Shell extension and fast Rust command-line tool that show your Google Antigravity quotas and Claude Code usage limits in the top panel. It automatically shows whichever tool you prompted most recently, and switches its logo to match.

<p>
  <img src="extension/icons/antigravity.svg" alt="Antigravity logo" height="64">
  &nbsp;&nbsp;
  <img src="extension/icons/claude.svg" alt="Claude logo" height="64">
</p>

The extension appears as **Token Watcher for Antigravity & Claude** in the GNOME Extensions app. Its UUID and settings keep their original `antigravity-token-watcher` names, so existing installs and settings carry over.

---

## Features

- **Automatic Claude / Antigravity Switching**: The logo, top bar percentage, and popup follow whichever tool received a prompt last. See [Claude Support](#claude-support) for how the choice is made.
- **Fast Rust Backend**: Built in Rust with local caching, so a normal poll from the extension (a cache hit) takes about 1.2 ms and 5 MiB of memory. See [Performance](#performance).
- **Antigravity: CLI first, IDE fallback**:
  - **CLI**: Runs the `agy` CLI's `/usage` and `/model` print-mode commands, which report live quotas and the active model without using any quota.
  - **SQLite fallback**: Without `agy`, or when it fails and there's no cached report from the last 7 days, reads quotas from the Antigravity IDE's `state.vscdb` with a built-in protobuf parser.
- **Claude Code**: Reads the session and weekly limits from `claude -p /usage`, which uses no tokens.
- **Top Bar Display**: Shows the remaining percentage, e.g. `76%`, `Flash: 76%`, or for Antigravity's two model pools `⚡ 76% | 🧠 13%`.
- **Instant Updates**: Watches `~/.claude/history.jsonl`, `~/.gemini/antigravity-cli/history.jsonl` and `state.vscdb` with `Gio.FileMonitor` (`inotify`), so the top bar updates as soon as you send a prompt.
- **Detailed Popup Menu**:
  - Account name, email, and plan (e.g. `Antigravity Starter Quota` or `Claude Pro`). For Antigravity, also the active model.
  - One card per limit: Antigravity's Gemini pool and Claude & GPT pool, or Claude's session and weekly limits.
  - Color-coded progress bars (green above 50%, amber above 20% up to 50%, red at 20% or below).
  - Time until each limit resets.
  - "Refresh" button, and "Open IDE" for Antigravity (hidden while Claude is shown).
- **Configurable Preferences**:
  - Display format: percentage only, model + percentage, dual view (both Antigravity pools, or Claude's first two limits), or icon only.
  - Primary Antigravity model (Flash, Pro, Sonnet). Claude always shows its tightest limit.
  - Panel position (right, center, left).
  - Low quota warning threshold.
  - Refresh interval.
- **Standalone CLI**: Can also be used directly from the terminal or in scripts via `token-watcher`.

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
├── extension/          # GNOME Shell extension (GNOME Shell 45–50, ESM)
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

## How the Antigravity SQLite Reader Works

This section describes the Antigravity IDE fallback, which reads quota details directly from local Antigravity IDE storage. It makes no network requests and takes about 1.4 ms per run. Fresh fetches through the `agy` and `claude` CLIs do go over the network (see [Performance](#performance)); Claude is covered in [Claude Support](#claude-support).

### 1. Read-Only SQLite State Access ([`src/db.rs`](src/db.rs))
- **Path Resolution**: Automatically targets Antigravity's global state store at `~/.config/Antigravity/User/globalStorage/state.vscdb` (or a custom path via `--db`).
- **Non-blocking Concurrency**: The database is opened using SQLite URI read-only mode (`file:<path>?mode=ro`) with `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI`. This prevents database locks or collisions while the Antigravity IDE is actively writing tokens and chat histories to the database.
- **State Query**: Executes a targeted query against `ItemTable` for two synchronization keys:
  - `antigravityUnifiedStateSync.userStatus`: Houses serialized user account info, subscription tier, and real-time model quota structures.
  - `antigravityUnifiedStateSync.modelPreferences`: Houses agent model preferences and active model state.

### 2. Schema-less Protobuf Wire Parser ([`src/parser.rs`](src/parser.rs))
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
- Extracts and maps the active model ID to mark `is_selected: true` on the matching quota record (shown with an asterisk `*` in terminal reports and a highlighted card in the popup).

## Output Modes ([`src/main.rs`](src/main.rs), [`src/models.rs`](src/models.rs))

These apply to both Claude and Antigravity reports.
- **`--json`**: Serializes `TokenReport` as JSON, including a `provider` field (`"claude"` or `"antigravity"`). The GNOME Shell extension reads this through `Gio.Subprocess`.
- **`--summary`**: Prints one line for status bars like tmux, Polybar, Waybar, or i3blocks, e.g. `Claude Session: 78% | Week (all models): 93%`, or for Antigravity `Gemini: 72% | Claude: 100%` (the second figure is Antigravity's Claude & GPT pool, not Claude Code).
- **Default (Terminal)**: Prints a formatted report with Unicode progress bars (`█` / `░`) and reset countdowns.

---

## Installation & Setup

### Requirements
- A Rust toolchain (`cargo`) and `glib-compile-schemas` to build.
- GNOME Shell 45–50.
- At least one of:
  - **Antigravity**: the `agy` CLI, or the Antigravity IDE (for the SQLite fallback).
  - **Claude Code**: the `claude` CLI, logged in with a subscription (usage limits only exist for subscription plans). It must be in `~/.local/bin`, on your `PATH`, or in `/usr/local/bin` or `/usr/bin`.

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

### 3. Activate it in GNOME Shell
GNOME Shell only picks up newly installed extension files when it restarts:
1. On **Wayland**, log out and back in. On **X11**, press Alt+F2, type `r`, and press Enter.
2. Enable the extension:
   ```bash
   make enable
   ```
   Or toggle it in the **Extensions** app (`gnome-extensions-app`).

After updating, run `make install` again and repeat step 1.

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

# Pick a tool instead of the most recently prompted one
./target/release/token-watcher --provider claude
./target/release/token-watcher --provider antigravity
```

Other options: `--refresh` skips the cache, `--ttl <SECONDS>` sets how long a cached report is used (default 60), and `--db <PATH>` reads a specific `state.vscdb` directly. Run with `--help` for the full list.

Example output for Claude Code:
```
==========================================================
                   CLAUDE USAGE WATCHER                   
==========================================================
User:         ada <ada@example.com>
Plan:         Claude Pro
----------------------------------------------------------
MODEL / GROUP                          REMAIN       RESETS IN  STATUS    
----------------------------------------------------------
 Session                              78.0% [████████░░]     2h 49m          
 Week (all models)                    93.0% [█████████░]     1d 13h          
==========================================================
```

Example output for Antigravity (via the `agy` CLI):
```
==========================================================
            ANTIGRAVITY TOKEN & QUOTA WATCHER             
==========================================================
User:         Ada Lovelace <ada@example.com>
Plan:         Antigravity Starter Quota
Active Model: Gemini 3.8 Flash (Medium)
----------------------------------------------------------
MODEL / GROUP                          REMAIN       RESETS IN  STATUS    
----------------------------------------------------------
*Gemini (Flash & Pro)                 71.5% [███████░░░]        17m  Fast    
 Claude & GPT (Sonnet, Opus, GPT-OSS) 100.0% [██████████]      7d 0h  Thinking
==========================================================
```

When Antigravity data comes from the IDE's SQLite database instead, each model is listed separately (e.g. `Gemini 3.6 Flash (High)`, `Claude Sonnet 4.6 (Thinking)`) rather than grouped into the two pools.
