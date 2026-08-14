# Agent Note: Desktop PATH bridge for Host CLIs

Status: implemented

English | [中文](2026-08-14-desktop-path-bridge.zh.md)

## Problem

The desktop Host starts `node apps/cli/lib/bin.js web` without putting the selected toolchain on PATH. Product code then looks up bare names: the plugin catalog `spawn`s `dsh`, `dsh plugin` spawn-syncs `pnpm`, bash-local runs `bash -c`, MCP stdio examples use `npx`, and agents invoke `dsh` or `git`. A machine that only has the private runtime, or has Git for Windows off PATH, fails those lookups even though the files exist.

On Windows those lookups are stricter than a user terminal. Node `spawn('dsh')` without a shell does not run `.cmd` after the CVE-2024-27980 hardening, and an extensionless `dsh` file in the same directory is chosen first and fails with `ENOENT`. `spawnSync('pnpm')` uses `PATHEXT`, so a PowerShell `pnpm.ps1` is not a hit. Persisting the user Path by spawning PowerShell from the GUI process often fails silently, so a new terminal never sees `dsh` either.

## Decision

**The shell writes spawnable `dsh` and `pnpm` shims and prepends every required CLI directory onto the Host PATH.** `%APPDATA%\DeepSeek Harness\bin` (or the platform application-data `bin`) is first on that PATH and receives:

- `dsh.cmd` (and a Unix `dsh` script) that prepend the selected Node and pnpm directories onto `PATH`, then exec the selected Node and `apps/cli/lib/bin.js`
- `dsh-launch.json` naming that Node, CLI entry, `$DSH_HOME`, and the same `pathPrepend` directories
- `dsh.exe` — a hard link or copy of the desktop binary; when `argv0` is `dsh`, the process attaches a visible parent console when one exists, applies the sidecar's `pathPrepend` to `PATH`, and execs the sidecar Node with `CREATE_NO_WINDOW` when that console is missing or hidden, instead of opening the GUI
- `pnpm.cmd` (Unix `pnpm`) that runs `node …/pnpm.cjs` when that file sits next to the selected pnpm, otherwise `call`s a `.cmd` / `.bat` or execs the selected binary

Every `dsh` entry point therefore resolves the provisioned pnpm first, so `dsh plugin` from any terminal uses the same pnpm major as the desktop and one profile stays on one pnpm store; a sidecar from an older build without `pathPrepend` parses with an empty prepend and behaves as before.

Windows does **not** write an extensionless `dsh` file: that name shadows `dsh.cmd` / `dsh.exe` for Node `spawn('dsh')`. A leftover extensionless file is deleted when the shims are rewritten.

The Host child PATH starts from the discovery PATH (process PATH plus, on Windows, durable user and machine Path), then prepends that bin directory, the selected Node directory (`node` / `npm` / `npx`), the selected pnpm directory, and well-known Git `cmd`/`bin` directories when `git` or `bash` is not already resolvable on that discovery PATH. The same merged PATH is applied to the desktop process. ripgrep stays packaged inside `@vscode/ripgrep` and is not added. Optional third-party CLIs (`claude`, `codex`, `tmux`) are not implanted.

A host pnpm counts as present only when it is directly spawnable (`.exe` / `.cmd` / `.bat` / `.com` on Windows). A `.ps1` is ignored so the private `pnpm.cmd` or the bin shim is used instead.

**The user PATH receives only what is still missing.** Windows appends the shim directory to the HKCU user Path through the registry and broadcasts `WM_SETTINGCHANGE`, and appends the Node or pnpm directory only when that command is still absent from the discovery PATH as a spawnable file. Unix copies the `dsh` shim to `~/.local/bin` and, when that directory is not already on PATH, appends a marked `export PATH` block to `~/.zprofile` (macOS) or `~/.profile` (Linux). Existing Path entries are never reordered or removed. A persist failure is logged and does not block Host start.

This sits on [desktop host toolchain scan and home adoption](2026-08-14-desktop-host-env-and-home-adoption.md): the same selected Node and pnpm feed both the scan and the shims.

## Alternatives considered

**Persist every discovered directory, including Git, onto the user Path.** Rejected because rewriting the user Path for a third-party install is surprising; the Host process PATH is enough for agent `git`/`bash` calls.

**Install `dsh` as a global npm package.** Rejected because the desktop already owns a specific CLI entry in the bundled tree; a global install would resolve a different version.

**Change bash-local, `dsh plugin`, or MCP to take absolute paths.** Rejected because those lookups are product contracts on PATH; the desktop fork must not edit `packages/`.

**Ship only `dsh.cmd` plus an extensionless Unix script on Windows.** Rejected: Node `spawn('dsh')` from the plugin catalog cannot run `.cmd` without a shell, and the extensionless file produces `spawn dsh ENOENT`.

**Persist the user Path by spawning PowerShell `SetEnvironmentVariable`.** Rejected after the GUI process often failed that spawn; HKCU `Environment` plus `WM_SETTINGCHANGE` is the durable write.

## Consequences

A new terminal may still need a restart before Explorer rebuilds its Path. The Host itself does not: it receives the merged PATH at spawn, and in-app `spawn('dsh')` / `spawnSync('pnpm')` resolve the bin shims. A later toolchain rescan overwrites the shims so they keep pointing at the selected Node. `dsh.exe` is refreshed from the running desktop binary; a file-in-use refresh is skipped and logged. Machines without Git still have no `git` or `bash` until the user installs them.
