# Agent Note: Desktop rc.7 provisioning failure and unbootable fallback

Status: implemented

English | [中文](2026-08-19-desktop-rc7-provision-fallback.zh.md)

## Problem

The bundled `pnpm-workspace.yaml` was generated from a hardcoded template whose `patchedDependencies` still named `node-pty@1.1.0`. Upstream `0.1.0-rc.7` moved the patch to `node-pty@1.2.0-beta.15`, and pnpm treats a declared-but-unused patch as a hard install error, so every first-launch `pnpm install --prod --no-frozen-lockfile` after the rc.7-0.1 update failed with `ERR_PNPM_UNUSED_PATCH`. Recovery then made the failure permanent: `find_existing_harness` accepted any tree containing the prebuilt CLI entry and ordered candidates by hash name, so it selected the freshly seeded, dependency-less tree over the previously working rc.5 tree; the Host exited with `ERR_MODULE_NOT_FOUND` and the application could not start. The install and download steps also carried no deadline, so a wedged registry or subprocess parked the splash indefinitely.

## Decision

The bundle script derives the bundled workspace file from the repository's `pnpm-workspace.yaml`, replacing only the `packages:` membership and copying `patchedDependencies`, `allowBuilds`, and every other section verbatim, so the declarations always match the source tree the bundle ships. A harness tree counts as bootable only when both `apps/cli/lib/bin.js` and `node_modules/.pnpm` exist; the bundle-hash recovery candidate and `find_existing_harness` accept only bootable trees, ordered newest by modification time. `pnpm install`, the pnpm self-install, and the Node archive download carry explicit deadlines of 20, 10, and 15 minutes; expiry fails the step into the existing recovery path instead of parking the splash. After a successful provision, only the newest three `harness-versions` trees are kept and older ones are deleted with per-directory failure tolerance.

This note owns the bundled workspace derivation, fallback validity, step deadlines, and tree cleanup. The provisioning model and update flow remain owned by [cross-platform desktop source provisioning](../feature/2026-08-14-cross-platform-desktop-source-provisioning.md).

## Alternatives considered

**Pin the bundle template to the rc.7 patch set.** Rejected because the next upstream patch bump recreates the same hard failure; deriving from the repository file removes the copy that can drift.

**Strip unused patch entries during bundling.** Rejected because it needs a dependency-graph computation in the bundler; copying the declarations verbatim is exact and one pass.

**Keep accepting any tree with the CLI entry and retry `pnpm install` on it.** Rejected because a seeded tree always contains the prebuilt CLI, so every launch retries the install on the same broken tree; the installed dependency store is what separates a bootable tree from a seeded one.

**Treat a stalled install as ongoing progress.** Rejected because the splash has no cancel path; a deadline that fails into recovery reuses the last working tree instead.

**Garbage-collect every superseded tree immediately.** Rejected because an older running Host may still hold files in the previous tree during an update; keeping three trees covers the active tree, the fallback, and one spare.

## Consequences

A desktop update whose provisioning fails now boots against the most recent tree that actually ran instead of bricking startup. Boot-time installs and downloads can no longer hang past their deadlines. Disk usage stops growing with every release once a provision succeeds; a tree held by an older process survives one more update cycle. The bundled workspace file changes whenever the repository's does, so bundle hashes change with upstream workspace edits even when the shipped source does not. Desktop crate tests pin bootable-tree validity, recovery ordering, and cleanup; a bundler test pins the workspace derivation, and the exact first-launch install command is verified against the regenerated bundle before release.
