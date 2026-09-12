# DSH One-Click Launcher

[![Release](https://img.shields.io/github/v/release/zhubingzuo/dsh-oneclick-launcher)](https://github.com/zhubingzuo/dsh-oneclick-launcher/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**English** | [简体中文](README.md)

A **double-click-to-run** launcher for the DeepSeek Harness web UI on Windows: it silently starts the
harness, opens a **brand-new, clean Chrome window** on the DSH page with **no console window ever
showing up**, and stops the background service when you close that window — leaving no stray processes.

> An unofficial launcher for the [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
> (`@deepseek-ai/dsh`) web UI.
>
> It is built to **survive DSH releases**: the address comes from dsh's own output (no guessed ports,
> no hard-coded URL) and the way dsh is started lives in an editable config file — a normal DSH update
> needs **no rebuild** of this launcher.

## Download

Grab the exe from the [latest release](https://github.com/zhubingzuo/dsh-oneclick-launcher/releases/latest),
or build it yourself (see [Building from source](#building-from-source-zero-dependencies-offline)):

```
target\release\dsh-launcher.exe
```

Copy it anywhere (Desktop, USB stick, …) and double-click. On first run it creates a
`dsh-launcher.conf` next to the exe (see [Configuration](#configuration-dsh-launcherconf)).

## Features

- **Double-click and go, no console**: the exe is a GUI-subsystem binary and every child process is
  created with `CREATE_NO_WINDOW`, so no black window ever flashes
- **Exactly one clean window**: `dsh web` opens a browser of its own by default; the launcher turns
  that off with the official `--no-open` flag, so only **one** window is opened by the launcher
- **A fresh, isolated Chrome window**: launched with its own throwaway profile directory
  (`--user-data-dir`) plus `--new-window`, `--no-first-run`, `--disable-session-crashed-bubble` —
  an ordinary window with an address bar that contains **only the DSH tab**, never your everyday
  Chrome bookmarks, extensions, logins or other tabs
- **Close-to-stop, nothing left behind**: closing the launcher's Chrome window ends the dsh web
  process tree it started (Windows Job Object `KILL_ON_JOB_CLOSE` + `taskkill /T`, belt and braces)
- **Never disturbs someone else's service**: if the preferred port is already served, the launcher
  reuses it when it needs **no token** (older DSH) and otherwise starts its own instance on a free
  port. **It never kills a service it did not start**
- **Resilient to version changes**: the page URL is taken from the tokenized line dsh prints, and the
  command, arguments, port, output marker and timeout are all editable — when DSH changes how it
  starts, you **edit text instead of rebuilding**
- **Single instance**: double-clicking again does not start a second copy
- **Actionable errors**: any failure raises a message box with the log paths and the config keys to edit

## Why a DSH update usually needs no rebuild

| Possible change in DSH | How the launcher copes |
| --- | --- |
| The page port changed | The port is not hard-coded: the URL dsh prints is always used (`--port 0` lets the OS pick when needed) |
| The page gained a token / auth fence | The tokenized URL is read from dsh's stdout (the bare URL returns 401 and is never used) |
| Output wording / format changed | Parsed by the configured `url_marker` first, then by matching a loopback URL that carries a token |
| A flag was renamed/removed (e.g. `--no-open`) | Launch attempts fall back in order: full command → without `--port` → command only |
| The subcommand or package was renamed | Edit `command` / `extra_args` in `dsh-launcher.conf`; no rebuild |

## Configuration `dsh-launcher.conf`

Created on first run next to the exe (or in `%LOCALAPPDATA%\dsh-launcher\` when that directory is not
writable). Plain text with comments; changes apply immediately:

```ini
command = npx --yes @deepseek-ai/dsh web    # how to start the UI
extra_args = --no-open                      # arguments for the first attempt
port = 3080                                 # preferred port; 0 = always let dsh choose
url_marker = dsh web:                       # text that marks the line carrying the URL
timeout_secs = 300                          # readiness timeout
```

The environment variable `DSH_LAUNCHER_CONFIG` can point at a different config file.

## Behavior in detail

| Situation | What the launcher does |
| --- | --- |
| Preferred port is free | Starts `npx --yes @deepseek-ai/dsh web --no-open --port 3080`, reads the tokenized URL it prints, opens a fresh Chrome window |
| Preferred port serves an **older** DSH (no token) | **Reuses** that service and only opens the browser; on window close it exits and **leaves that service running** |
| Preferred port serves a **token-protected** DSH or anything else | Starts its **own** dsh web with `--port 0` (OS-assigned free port); on window close it stops only its own service |
| Another launcher is already running | The single-instance lock makes the second copy exit immediately |
| You close the Chrome window | Ends the dsh web process tree it started → removes the throwaway profile → exits |
| Node.js or Chrome missing, startup failure, timeout | Message box + detailed log paths + the config keys to change |

## Building from source (zero dependencies, offline-friendly)

Requirements: Windows + Rust (MSVC toolchain). The icon is embedded with the Windows SDK's `rc.exe`;
when `rc.exe` is missing the build still succeeds, just without an icon.

```powershell
cargo build --release             # needs the Windows SDK rc.exe for the embedded icon
cargo build --release --offline   # or force offline
```

## Regression tests

There are no unit tests; `scripts/regression.ps1` runs three end-to-end scenarios
(`cargo build --release` first):

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/regression.ps1
```

- **A** preferred port free → the launcher starts its own service, reads the tokenized URL, stops the
  service and releases the port when the window closes
- **B** preferred port taken by a **token-protected** dsh → the launcher falls back to `--port 0` and
  leaves the other service untouched
- **C** preferred port taken by a **token-less** service → the launcher reuses it, starts nothing and
  kills nothing

The script runs through `DSH_*` test switches, so it can run side by side with a launcher you are
already using.

## Application icon

`assets/icon.ico` is generated from DSH's official whale favicon: rounded DeepSeek-blue tile with a
white whale, in 9 sizes from 16 to 256 px. To regenerate it:

```powershell
# point at DSH's favicon.svg (found inside the @deepseek-ai/dsh package)
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/gen_icon.ps1 -SvgPath "C:\...\favicon.svg"
```

Without `-SvgPath` the script probes the npm global install directory
(`%APPDATA%\npm\node_modules\@deepseek-ai\dsh\...`) for `favicon.svg`.

## Troubleshooting and logs

Every failure points at the logs in its message box:

- Launcher log: `%LOCALAPPDATA%\dsh-launcher\launcher.log`
- Service output: `%LOCALAPPDATA%\dsh-launcher\server.log` (dsh's stdout + stderr)

| Symptom | What to do |
| --- | --- |
| "cannot start the DSH service" | Make sure Node.js is installed and on PATH; the first run may need network access to fetch the dsh package |
| "cannot open Chrome" | Install Google Chrome, or open the URL from the log manually |
| "could not obtain an access token" | A DSH release changed the output format: check `url_marker` in the config file |
| Startup times out | Inspect `server.log`, or run the configured `command` in a terminal; raise `timeout_secs` if needed |
| Two browser windows appear | Make sure you run the latest build (older builds lacked `--no-open`, so dsh opened a browser itself) |

## Repository layout

```
dsh-launcher/
├─ assets/icon.ico              # application icon (multi-size)
├─ scripts/gen_icon.ps1         # icon generator
├─ scripts/regression.ps1       # end-to-end regression scenarios A/B/C
├─ scripts/test-stub-server.ps1 # token-less stub service used by scenario C
├─ build.rs                     # zero-dependency icon embedding (rc.exe)
├─ src/main.rs                  # the whole program (std + a little hand-written Win32 FFI)
├─ AGENTS.md / HANDOFF.md       # project conventions / handoff notes (Chinese)
├─ Cargo.toml / Cargo.lock
├─ LICENSE
├─ README.md                    # 简体中文
└─ README.en.md                 # this file
```

## Test switches (not needed for normal use)

| Environment variable | Purpose |
| --- | --- |
| `DSH_LAUNCHER_CONFIG` | Config file path (default: `dsh-launcher.conf` next to the exe) |
| `DSH_DATA_DIR` | Log/temp directory (default `%LOCALAPPDATA%\dsh-launcher`) |
| `DSH_PORT` | Override the preferred port (default from the config file, 3080) |
| `DSH_SERVER_EXTRA` | Replace the configured `command` entirely (used by the regression suite) |
| `DSH_FAKE_CHROME` | Use ping instead of Chrome to simulate "browser closes after about N seconds" |
| `DSH_NO_UI` | `1` suppresses message boxes, logging only |
| `DSH_READY_TIMEOUT_SECS` | Override the readiness timeout |
| `DSH_CHROME` | Path to chrome.exe |
| `DSH_SKIP_SINGLE_INSTANCE` | `1` skips the single-instance check (lets the regression run alongside a live instance) |

## License

[MIT](LICENSE) © 2026 zhubingzuo

## Disclaimer

This is an unofficial launcher. It is not affiliated with DeepSeek; DeepSeek Harness and its logos
belong to their respective owners.
