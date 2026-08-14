# Agent Note: Desktop host toolchain scan and home adoption

Status: implemented

English | [中文](2026-08-14-desktop-host-env-and-home-adoption.zh.md)

## Problem

The desktop installer isolates `DSH_HOME` under application data and downloads Node even when the machine already has a compatible toolchain or a CLI/Web Harness home. Users who already ran `dsh web` or `dsh` lose their sessions and API keys on first desktop launch, and first launch repeats a multi-minute runtime fetch.

## Decision

**Provisioning scans the host before any mirror download.** The shell probes the private runtime, `PATH`, and well-known Node/pnpm locations. A Node that satisfies `^22.19 || >=24` is reused for Host startup and `pnpm install`. A usable pnpm is reused the same way. The private Node archive is fetched only when the scan finds no compatible Node; pnpm is installed through the selected Node only when no usable pnpm exists. This extends [cross-platform desktop source provisioning](2026-08-14-cross-platform-desktop-source-provisioning.md) without changing the fallback download.

**The Host adopts an existing Harness home instead of always using `dsh-home/`.** Selection order is `$DSH_HOME` when that directory already holds Harness data, then `~/.dsh`, then the isolated application-data home. A directory counts as a Harness home when it contains `sessions`, `.credentials.yaml`, `.env`, `profiles`, or `settings.yaml` / `.yml` / `.json`. Missing files and session directories from every other discovered home are copied into the selected home; existing files are left in place. `desktop-overlay` is never imported because the shell regenerates it.

## Alternatives considered

**Keep the isolated `dsh-home/` and copy CLI data into it.** Rejected because later CLI or `dsh web` launches would not see desktop sessions, and keys would drift between two homes.

**Always set `DSH_HOME` to `~/.dsh`, even when that directory is empty.** Rejected because a fresh desktop-only user would then write into the default CLI home without having opted into a shared tree.

**Use any Node on `PATH` without an engine check.** Rejected because Node 18/20 and Node 22 below 22.19 fail the workspace engines range and would break Host startup.

**Merge credential YAML documents key-by-key.** Rejected because a structural merge can corrupt the managed store. Missing-file copy keeps CLI keys when both homes already have `.credentials.yaml`, and copies the desktop file when the selected home has none.

## Consequences

A machine with a compatible Node skips the runtime archive download; `pnpm install` of the bundled tree still runs when that tree has no `node_modules`. Desktop and CLI share one home when `~/.dsh` already exists, including sessions and keys. The overlay plugin is written into the selected home. A host Node that later disappears fails the next boot's scan and falls back to the private runtime. The selected binaries are then exposed on PATH by [the desktop PATH bridge](2026-08-14-desktop-path-bridge.md).
