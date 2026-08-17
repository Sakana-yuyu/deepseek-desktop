# Agent Note: Desktop tray install of the Sakana plugin catalog

Status: implemented

English | [中文](2026-08-17-desktop-tray-plugin-catalog.zh.md)

## Problem

The desktop Host `web` profile does not include a plugin catalog. Installing [dsh-plugins](https://github.com/Sakana-yuyu/dsh-plugins) requires running `dsh plugin --profile web add github:Sakana-yuyu/dsh-plugins` against the **live** Host home (Windows desktop `$DSH_HOME`, or distro `~/.dsh` in WSL). Asking the operator to open a terminal and pick the right `dsh` / `DSH_HOME` is easy to get wrong, especially after the PATH bridge and WSL split.

## Decision

**One tray item installs that one catalog into the running Host profile, then restarts.** The label is 安装插件库 / Install plugin catalog. The spec is fixed: `github:Sakana-yuyu/dsh-plugins` (package `dsh-plugins-catalog`, ships built `index.js`, no `prepare` script). The shell runs `node <cli> plugin --profile web add <spec>` with the same Node, CLI, `DSH_HOME`, and PATH the Host already uses. WSL uses `wsl.exe -d <distro> --exec` and Linux Node; it never runs `node.exe`. A second click while an install is running is ignored. Success toasts and calls the same `request_restart` as the tray Restart item so the Host reloads the new profile layer. Failure toasts the process tail and does not restart. The item is disabled in effect until `DesktopRuntime` exists (splash still up). `packages/` is unchanged.

## Alternatives considered

**Open the GitHub page from the tray.** Rejected because the operator still has to run `dsh plugin` against the correct home.

**Prompt for an arbitrary git spec.** Rejected: this delivery is one known catalog; a generic installer would need `allowBuilds` handling for packages that ship sources only.

**Install without restarting.** Rejected because `dsh web` has already composed the `web` profile; new bundles are not visible until the Host starts again.

## Consequences

Windows and WSL homes stay separate: installing on Windows does not add the catalog to a WSL `~/.dsh`. GitHub must be reachable; a blocked `github.com` fails the tray item with the pnpm/git tail. Re-running the item updates the existing git dependency.
