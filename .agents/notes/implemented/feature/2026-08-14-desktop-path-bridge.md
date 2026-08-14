# Agent Note: Desktop PATH bridge for Host CLIs

Status: implemented

English | [中文](2026-08-14-desktop-path-bridge.zh.md)

## Problem

The desktop Host starts `node apps/cli/lib/bin.js web` without putting the selected toolchain on PATH. Product code then looks up bare names: `dsh plugin` spawn-syncs `pnpm`, bash-local runs `bash -c`, MCP stdio examples use `npx`, and agents invoke `dsh` or `git`. A machine that only has the private runtime, or has Git for Windows off PATH, fails those lookups even though the files exist.

## Decision

**The shell writes a `dsh` shim and prepends every required CLI directory onto the Host PATH.** `%APPDATA%\DeepSeek Harness\bin` (or the platform application-data `bin`) receives `dsh.cmd` plus a `dsh` shell script that exec the selected Node and `apps/cli/lib/bin.js`. The Host child PATH starts from the discovery PATH (process PATH plus, on Windows, durable user and machine Path), then prepends that bin directory, the selected Node directory (`node` / `npm` / `npx`), the selected pnpm directory, and well-known Git `cmd`/`bin` directories when `git` or `bash` is not already resolvable on that discovery PATH. ripgrep stays packaged inside `@vscode/ripgrep` and is not added. Optional third-party CLIs (`claude`, `codex`, `tmux`) are not implanted.

**The user PATH receives only what is still missing.** Windows appends the shim directory to the HKCU user Path, and appends the Node or pnpm directory only when that command is still absent from the discovery PATH. Unix copies the `dsh` shim to `~/.local/bin` and, when that directory is not already on PATH, appends a marked `export PATH` block to `~/.zprofile` (macOS) or `~/.profile` (Linux). Existing Path entries are never reordered or removed.

This sits on [desktop host toolchain scan and home adoption](2026-08-14-desktop-host-env-and-home-adoption.md): the same selected Node and pnpm feed both the scan and the shims.

## Alternatives considered

**Persist every discovered directory, including Git, onto the user Path.** Rejected because rewriting the user Path for a third-party install is surprising; the Host process PATH is enough for agent `git`/`bash` calls.

**Install `dsh` as a global npm package.** Rejected because the desktop already owns a specific CLI entry in the bundled tree; a global install would resolve a different version.

**Change bash-local, `dsh plugin`, or MCP to take absolute paths.** Rejected because those lookups are product contracts on PATH; the desktop fork must not edit `packages/`.

## Consequences

A new terminal may need a restart before `dsh` appears on the user Path. The Host itself does not: it receives the merged PATH at spawn. A later toolchain rescan overwrites the shims so they keep pointing at the selected Node. Machines without Git still have no `git` or `bash` until the user installs them.
