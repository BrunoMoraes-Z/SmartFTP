# SmartFTP

<img src="assets/app-icon.png" width="72" align="right" alt="SmartFTP icon">

A desktop FTP / FTPS / SFTP client written in Rust with [GPUI](https://www.gpui.rs/), with a built-in **MCP server** so AI agents can work in the same sessions you do.

[![CI](https://github.com/BrunoMoraes-Z/SmartFTP/actions/workflows/ci.yml/badge.svg)](https://github.com/BrunoMoraes-Z/SmartFTP/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/BrunoMoraes-Z/SmartFTP)](https://github.com/BrunoMoraes-Z/SmartFTP/releases/latest)

<p align="center">
  <a href="docs/media/smartftp-tour.mp4"><img src="docs/media/smartftp-tour.gif" alt="Animated tour: connecting, transferring files, organizing sites, and an AI agent deploying through MCP" width="880"></a>
  <br>
  <sub>A one-minute tour. <a href="docs/media/smartftp-tour.mp4">Watch it as video (MP4)</a> or open <a href="docs/motion/index.html"><code>docs/motion/index.html</code></a> in a browser for the interactive version with chapters.</sub>
</p>

## Highlights

- **FTP, FTPS and SFTP** with password or private-key login, and host keys pinned on first use.
- **Two panes, local and remote.** Send files with the → / ← button on a row, a double-click, or drag and drop, including straight from Explorer.
- **Transfer queue like FileZilla's.** Add files to the queue and start them all, or one by one, when you are ready.
- **View / Edit.** Open a remote file in VS Code or any editor; when you save, SmartFTP offers to upload it and replace the original.
- **AI agents over MCP.** 18 tools to connect, browse, upload, download, read and write files. What an agent does shows up live in the window.
- **A connection rail like Discord's.** Sites are tiles with your own icon and color; drop one onto another to make a folder.
- **FileZilla-style settings**: timeouts, retries, reconnect, FTP modes, speed limits, simultaneous transfers and what to do when a file exists.
- **Themes and languages.** Six built-in themes plus your own TOML files; English, Brazilian Portuguese and Spanish.
- **Updates itself** from GitHub Releases, verified with SHA-256, always asking first.

## Install

Download the latest release from the [Releases page](https://github.com/BrunoMoraes-Z/SmartFTP/releases/latest):

| File | What it is |
|---|---|
| `SmartFTP-<version>-setup.exe` | Installer. Installs for your user only, no admin rights, with a Start menu entry, an optional desktop shortcut and an uninstaller. |
| `SmartFTP-<version>-windows-x86_64.zip` | Portable build: unzip and run `smartftp.exe`. |

On startup SmartFTP checks for a newer release and shows an **Update** button in the title bar. Click it to download, verify and restart into the new version. You can turn the check off in **Settings → General**.

Your settings and saved sites live in `%APPDATA%\SmartFTP`. The uninstaller asks whether to remove them.

## AI agents (MCP)

SmartFTP runs an MCP server while it is open. Open **Settings → MCP & Agents** for the endpoint, the token (reveal, copy or regenerate) and ready-to-copy client configs. For Claude Code:

```bash
claude mcp add --transport http smartftp http://127.0.0.1:7331/mcp --header "Authorization: Bearer <token>"
```

Then ask in plain language, for example *"upload ./dist to the staging site and check logs/deploy.log"*. The agent's session appears in the rail with a bot badge, and its transfers show up in your queue.

| Group | Tools |
|---|---|
| Connections | `list_sites`, `list_sessions`, `connect`, `disconnect` |
| Browsing | `list_dir`, `change_dir` |
| Remote files | `make_dir`, `delete`, `rename`, `chmod`, `read_file`, `write_file` |
| Transfers | `upload`, `download`, `process_queue`, `list_transfers`, `cancel_transfer` |
| Local | `list_local_dir` |

- `connect` takes a saved site or a URL such as `sftp://user@host/path`, and can save it as a new site.
- `upload` and `download` are recursive. By default they wait for completion and return a result per file. With `queue_only` they only add the transfers to the queue; `process_queue` starts them.
- `delete` needs `recursive` for non-empty folders.

You stay in control:

- A token is required, and the server listens on loopback only.
- Each tool can be switched off.
- Each site has an **Allow AI agents** toggle.
- Passwords are never returned to the agent.

Headless mode with no window: `smartftp --mcp-stdio`. It uses the same saved sites but keeps its own sessions.

## Using SmartFTP

### Connections

- **Saved sites** are tiles in the rail on the left. Hover to see the name; click to connect, or to switch to an open session.
- **Folders.** Drag tiles to reorder them. Drop a site onto another site to create a folder, or onto a folder to add it. Folders show a grid of their sites and open in place when clicked.
- **Right-click** a site to connect, edit, change its icon and color, take it out of a folder or delete it. Right-click a folder to collapse, rename, ungroup or recolor it.
- **Quick connect**: the ⚡ button takes a URL like `sftp://user:password@host:22/path` without saving it.
- Each site can have a **default local and remote folder**, set in the site editor.

### Files and transfers

- **Moving files** (files dragged in from Explorer can be dropped on the remote pane or on one of its folders):
  - the → / ← button on a row;
  - a double-click on a file;
  - the **Upload to… / Download to…** button for the current selection;
  - dragging between the panes, or onto a specific folder.
- **Path bar**: click a breadcrumb to jump to it. Click the empty part of the bar, or press `Ctrl+L`, to type a path. Remote paths can be relative and `..` works. The local pane also has a folder picker.
- **Compare badges** (`▲ newer`, `older`, `only here`) show how the two sides differ. Timestamps are kept on upload and download, so the badges stay meaningful.
- **Transfers**: progress, speed, cancel and retry. The **Activity** tab shows who did what, you or an agent.
- **Queue**: choose **Add to queue**, from the right-click menu or the selection bar, to line transfers up without starting them. The **Queue** tab lists them. **Process queue** starts them all; ▶ starts a single one and ✕ removes it.
- **View / Edit** (`F4`, or right-click a file):
  - remote files are downloaded to a temporary folder and opened in your editor;
  - whenever you save, SmartFTP asks whether to upload the file and replace the one on the server. To skip the question for a site, turn on **Upload edited files automatically** in its settings;
  - the **Editing** tab lists the files you have open, with **Upload now** and **Stop editing**;
  - pick the editor in **Settings → General → File editor**: the system default, a detected editor (VS Code, Cursor, Notepad++, Sublime Text, Notepad) or any program. `{file}` in the command is replaced by the path.

| Key | Action |
|---|---|
| `Enter` | Open folder / transfer selection |
| `Backspace` | Parent folder |
| `Ctrl+L` | Type a path |
| `F5` | Refresh |
| `F2` | Rename |
| `F4` | View / Edit |
| `F7` | New folder |
| `Delete` | Delete |
| `Ctrl+A` | Select all |
| `Esc` | Close dialog or menu |

### Settings

- **General**:
  - language;
  - file editor for View / Edit;
  - updates;
  - hidden files;
  - double-click action;
  - delete confirmation;
  - opening the transfer panel when a transfer starts.
- **Connection**:
  - timeout;
  - maximum retries;
  - delay between failed attempts;
  - automatic reconnect;
  - SFTP keep-alive;
  - FTP passive / extended passive / active mode, and a fix for passive mode behind NAT.
- **Transfers**:
  - simultaneous transfers;
  - retries;
  - download and upload speed limits;
  - timestamp preservation;
  - what to do when a file already exists: overwrite, overwrite if newer, overwrite if different, rename or skip.
- **Appearance**: themes and System / Light / Dark mode.
- **MCP & Agents**: see above.

### Themes

Built-in themes: Graphite Dark/Light, GitHub Dark/Light, One Dark Pro and Dracula. Your own themes go in `%APPDATA%\SmartFTP\themes\*.toml`. **Settings → Appearance → Duplicate current** copies the active theme there and opens it; edit the colors and press **Reload**.

```toml
name = "My Theme"
appearance = "dark"        # or "light"

[colors]
bg = "#282A36"             # window background
surface = "#21222C"        # panels, rail, title bar
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

The built-in themes in [`themes/`](themes/) are good starting points.

### Languages

The interface comes in English, Brazilian Portuguese and Spanish. SmartFTP follows the system language when it is one of these and falls back to English otherwise. Change it any time in **Settings → General**.

## Development

### Build

The project uses [just](https://github.com/casey/just) with PowerShell 7 (`pwsh`), no extra modules:

```powershell
just build       # release build, copied to ./dist
just installer   # build + dist/SmartFTP-<version>-setup.exe (needs Inno Setup 6)
just package     # build + portable zip in ./dist
just run         # debug build
just check       # rustfmt, clippy and tests (same as CI)
just             # list every recipe
```

Without just: `cargo run --release`. Rust 1.88+ (edition 2024) is required. Crypto uses `ring`, so no OpenSSL or NASM is needed. To build the installer locally, install Inno Setup with `winget install JRSoftware.InnoSetup`.

### Releases

The version lives in `Cargo.toml`:

```powershell
just release minor   # or patch / major / 1.4.0: bumps Cargo.toml, commits and tags vX.Y.Z
just publish         # pushes the commit and the tag
```

A pushed `vX.Y.Z` tag runs `.github/workflows/release.yml`. The workflow checks that the tag matches `Cargo.toml`, runs the tests, builds the app and the installer, and publishes a GitHub Release with:

- the installer;
- the portable zip;
- `smartftp-windows-x86_64.exe` and its `.sha256`, which the in-app updater uses.

`.github/workflows/ci.yml` runs formatting, clippy and tests on every push and pull request.

### Testing against local servers

```powershell
just servers-up     # SFTP on :2222 and FTP on :2121 in Docker, user test / password test
just servers-down
```

Connect with quick connect to `sftp://test:test@127.0.0.1:2222/upload` or `ftp://test:test@127.0.0.1:2121`. To keep test data away from your real sites, run `just run-sandbox` or set `SMARTFTP_HOME=<folder>`.

### The tour video

`docs/motion/index.html` is the animation (GSAP). After changing it, regenerate the GIF and MP4 with Chrome and ffmpeg installed:

```powershell
cd docs/motion/recorder; npm install; node record.mjs
```

### Project layout

```
src/core/      protocols and state (no UI): config, FTP (suppaftp), SFTP (russh + russh-sftp),
               sessions, reconnect, transfer queue, rate limits, activity log
src/mcp.rs     MCP server (rmcp): Streamable HTTP + stdio
src/ui/        GPUI interface: workspace, rail, panes + queue, dialogs, settings, themes,
               title bar, updates, text input
src/i18n.rs    translations (en, pt-BR, es) and language detection
src/update.rs  self-update from GitHub Releases
installer/     Inno Setup script
themes/        built-in themes
assets/        app icon and Lucide icons (ISC license)
design/        OpenPen prototype and app icon source
docs/          tour animation, recorder and rendered media
```

The UI and the MCP server share one `Core`, held in an `Arc` on a Tokio runtime. The UI picks up changes by polling a version counter.

## Known limitations

- Passwords are stored in plain text in `config.json` in your user profile. Moving them to the OS keychain is still to do.
- There is no implicit FTPS (port 990) and no transfer resume.
- When a server's host key changes, the connection is refused. To accept the new key, remove its entry from `known_hosts.json`; there is no prompt for this yet.
- On FTP servers without `MLSD`, `LIST` timestamps are only accurate to the minute or the day. The compare badges allow for this.
- Release builds are produced for Windows only.
