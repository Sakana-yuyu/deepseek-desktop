# Agent Note: Desktop repeat boot reuses a recorded host toolchain

Status: implemented

English | [中文](2026-08-17-desktop-repeat-boot-host-toolchain.zh.md)

## Problem

The desktop ready-manifest skip required the private `runtime/node` and `runtime/pnpm-global` files to exist, then returned those private paths. [Host toolchain scan](../feature/2026-08-14-desktop-host-env-and-home-adoption.md) reuses a compatible Node and pnpm on the machine, so those private files are often never created. Every later launch then deleted the hash-addressed harness tree (including `node_modules`) and ran `pnpm install --prod` again. Operators with Node `^22.19 || >=24` already installed therefore sat on the splash for minutes on every start. The PATH-bridge `dsh.exe` shim was also unlinked and recopied from the desktop binary on every boot.

## Decision

`ready_toolchain` reads `nodePath` and `pnpmPath` from the runtime manifest. When the bundle hash still matches, `apps/cli/lib/bin.js` and `node_modules/.pnpm` exist, and those binaries (or the private fallback files) are still present, provision returns the recorded paths and skips the host scan, seed, and `pnpm install`. A harness directory that already has the CLI entry is not wiped; `pnpm install` runs only when `.pnpm` is missing. The manifest records `pnpmPath`. The `dsh.exe` shim is refreshed only when it is missing, a different size, or older than the desktop binary.

This note owns the repeat-boot skip. Host matching, home adoption, and first-run mirror fetch remain owned by [host toolchain scan and home adoption](../feature/2026-08-14-desktop-host-env-and-home-adoption.md) and [cross-platform source provisioning](../feature/2026-08-14-cross-platform-desktop-source-provisioning.md).

## Alternatives considered

**Always download a private Node so the old skip can keep requiring those files.** Rejected because host reuse is the host-env decision; forcing a private copy undoes it and still pays a first-boot archive download.

**Hash `node.exe` on every launch.** Rejected because the existing size check already identifies the recorded binary; hashing a large Node on the splash path is the cost that check was written to avoid.

**Keep deleting the harness tree on every provision to stay identical to the bundled resource.** Rejected because the destination is already hash-addressed; a new bundle is a new directory, and wiping a live tree is what made host-Node boots repeat `pnpm install`.

## Consequences

A later launch that still has the seeded tree and the recorded Node / pnpm skips seed and install, including when those binaries are host paths. An older manifest without `pnpmPath` uses the private pnpm file when it exists; otherwise one scan still skips seed and install when `node_modules/.pnpm` is present, then writes `pnpmPath`. A disappeared host Node fails the skip and falls through to scan. Desktop crate tests pin host-path reuse, missing-`node_modules` rejection, seed reuse, preferred-pnpm fallback, and the `dsh.exe` refresh skip.
