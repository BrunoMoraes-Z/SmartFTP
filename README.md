# SmartFTP

<img src="assets/app-icon.png" width="64" align="right" alt="SmartFTP icon">

A desktop FTP / FTPS / SFTP client written in Rust with [GPUI](https://www.gpui.rs/), with a built-in **MCP server** so AI agents can do everything you can do in the UI.

![main](design/export/01-main.png)

## Features

- **FTP, FTPS (explicit TLS) and SFTP**: password or private-key auth (SFTP). Host keys are pinned on first use (`known_hosts.json`).
- **Moving files between local and remote**:
  - the → / ← button on each row;
  - double-click a row;
  - the "Upload to … / Download to …" button for the current selection;
  - **drag and drop** between the panes, including onto a specific folder;
  - dragging files from your OS file manager onto the remote pane.
- **File management**: multi-select (Ctrl/Shift), a context menu (rename, permissions, delete, new folder/file, copy path), sortable columns and a dotfile toggle.
- **Path bar**: click a breadcrumb to jump to it. Click the bar's empty space (or press Ctrl+L) to type a path; remote paths may be relative and `..` is resolved. The local pane also has a folder picker.
- **Compare badges** (`▲ newer`, `older`, `only here`) between the two sides. Timestamps are preserved on upload and download.
- **Transfer queue** running 3 transfers at a time, with progress, speed, cancel and retry. An activity log shows who did what: you or an agent.
- **Connection rail**, Discord style:
  - every saved site is a tile; hover shows its name, click connects (or switches to the open session);
  - drag tiles to reorder; drop one site onto another to create a folder, or onto a folder to add it;
  - folders show a grid of their sites and expand in place when clicked;
  - right-click a site for connect, edit, icon & color, remove from folder and delete; right-click a folder to collapse, rename, ungroup or recolor it;
  - the list scrolls when it is taller than the window.
- **Per-connection icon and color**: from the rail's context menu or the site editor.
- **Themed title bar**: custom-drawn and following the active theme. Windows still handles dragging, snap layouts, double-click to maximize and the min/max/close buttons.
- **Settings modelled on FileZilla**:
  - General: hidden files, double-click action, delete confirmation, transfer panel behaviour.
  - Connection: timeout, number of retries, delay between failed attempts, automatic reconnect, SFTP keep-alive, FTP passive / extended passive / active mode, and a fix for passive-mode addresses behind NAT.
  - Transfers: maximum simultaneous transfers, retries for failed transfers, download and upload speed limits, timestamp preservation, and what to do when a file already exists (overwrite, overwrite if newer, overwrite if different, rename, skip).
- **Themes**: Graphite Dark/Light, GitHub Dark/Light, One Dark Pro and Dracula, with System / Light / Dark mode. Creating your own takes one TOML file.
- **MCP server** over Streamable HTTP at `127.0.0.1:7331/mcp`, protected by a bearer token, with 17 tools. Sessions an agent opens show up live in the window, and sessions you open are available to agents.

## Build and run

```bash
cargo run --release
```

Requires Rust 1.88+ (edition 2024). No OpenSSL or NASM is needed because crypto uses `ring`. On Windows the app icon is embedded into the executable by `build.rs`.

Keyboard shortcuts:

| Key | Action |
|---|---|
| `F5` | Refresh |
| `Enter` | Open / transfer |
| `Backspace` | Parent folder |
| `Delete` | Delete |
| `F2` | Rename |
| `F7` | New folder |
| `Ctrl+A` | Select all |
| `Ctrl+L` | Type a path |
| `Esc` | Close dialog / menu |

## MCP for agents

Open **Settings → MCP & Agents** to get the endpoint and the token (you can reveal, copy or regenerate it), plus ready-to-copy client configs. For Claude Code:

```bash
claude mcp add --transport http smartftp http://127.0.0.1:7331/mcp --header "Authorization: Bearer <token>"
```

Tools:

| Group | Tools |
|---|---|
| Connections | `list_sites`, `list_sessions`, `connect`, `disconnect` |
| Browsing | `list_dir`, `change_dir` |
| Remote files | `make_dir`, `delete`, `rename`, `chmod`, `read_file`, `write_file` |
| Transfers | `upload`, `download`, `list_transfers`, `cancel_transfer` |
| Local | `list_local_dir` |

- `connect` takes a saved site or a URL such as `sftp://user@host/path`, and can save it as a new site.
- `delete` needs `recursive` for non-empty folders.
- `upload` and `download` are recursive. By default they wait for completion and return a result per file.

Safety controls:
- A token is required, and the server listens on loopback only.
- Each tool can be switched off in settings.
- Each site has an "Allow AI agents" toggle.
- Passwords are never returned.

There is also a headless mode with no window: `smartftp --mcp-stdio`. It uses the same saved sites but keeps its own sessions.

## Themes

Themes live in `<config>/SmartFTP/themes/*.toml` (`%APPDATA%\SmartFTP\themes` on Windows). **Settings → Appearance → Duplicate current** copies the active theme into that folder and opens the copy for editing. Press **Reload** to apply your changes.

```toml
name = "My Theme"
appearance = "dark"        # or "light"

[colors]
bg = "#282A36"             # window background
surface = "#21222C"        # panels, sidebar, title bar
surface2 = "#343746"       # inputs, raised items
hover = "#3C3F52"
border = "#44475A"
ink = "#F8F8F2"            # primary text
muted = "#C3C5D9"
faint = "#6272A4"
accent = "#BD93F9"         # primary buttons
accent_ink = "#282A36"     # text on accent
accent_soft = "#BD93F926"  # selection (with alpha)
signal = "#FFB86C"         # upload progress, highlights
ok = "#50FA7B"
warn = "#F1FA8C"
err = "#FF5555"
folder = "#F1FA8C"
```

The built-in themes are in [`themes/`](themes/).

## Project layout

```
src/core/      protocols and state (no UI): sites/config, FTP (suppaftp), SFTP (russh + russh-sftp),
               sessions, transfer queue, activity log
src/mcp.rs     MCP server (rmcp): HTTP + stdio
src/ui/        GPUI: workspace (state, actions), rail (connection rail), panes (file panes + queue),
               dialogs, settings + settings_pages, theme, title bar, text input
design/        OpenPen prototype (smartftp.pen) and exported PNGs, including the app icon
themes/        built-in themes
assets/        app icon (.ico / .png) and Lucide icons (ISC license)
```

The UI and the MCP server share one `Core`, held in an `Arc` and running on a Tokio runtime. The UI watches for changes by polling a version counter.

## Testing against local servers

```bash
docker run -d --name smartftp-test-sftp -p 2222:22 atmoz/sftp test:test:1001::upload
docker run -d --name smartftp-test-ftp -p 2121:21 -p 21000-21010:21000-21010 \
  -e USERS="test|test" -e ADDRESS=127.0.0.1 -e MIN_PORT=21000 -e MAX_PORT=21010 delfer/alpine-ftp-server
```

To connect, use quick connect (the ⚡ button in the rail) with `sftp://test:test@127.0.0.1:2222/upload` or `ftp://test:test@127.0.0.1:2121`. To use a separate configuration, set `SMARTFTP_HOME=<folder>`.

## Known limitations

- Passwords are stored in plain text in `config.json` in your user profile. Moving them to the OS keychain is still to do.
- There is no implicit FTPS (port 990), no active-mode FTP and no transfer resume.
- On FTP servers without `MLSD`, `LIST` timestamps are only accurate to the minute or the day. The compare badges allow for this.
- Changing the MCP port requires a restart. Turning the server on or off takes effect immediately.
