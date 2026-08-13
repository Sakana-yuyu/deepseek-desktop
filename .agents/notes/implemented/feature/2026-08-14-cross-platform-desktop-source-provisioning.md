# Agent Note: Cross-platform desktop source provisioning

Status: implemented

English | [中文](2026-08-14-cross-platform-desktop-source-provisioning.zh.md)

## Problem

The desktop shell needs small installers for users who do not have a Harness development environment. Shipping the complete workspace dependency tree makes installers large and slow to assemble, while assuming a system Node installation makes startup depend on unmanaged host tools. A release also needs independently attributable artifacts for each supported operating system and architecture without publishing a partial platform set.

## Decision

**Desktop installers carry a trimmed source tree and built application artifacts, but no `node_modules`.** First launch copies that immutable resource into application data, downloads the Node archive selected for the compiled operating system and architecture, installs pnpm through that Node runtime, and runs `pnpm install --prod --no-frozen-lockfile` with `CI` removed. Mirror endpoints remain configurable through `DSH_NODE_MIRROR` and `DSH_NPM_REGISTRY`.

**The downloaded runtime owns all provisioning commands.** Windows x64 and x86 use the official zip layouts; macOS x64/arm64 and Linux x64/arm64 use tar.gz layouts. Archive entries must remain below the expected versioned Node directory. Tar extraction preserves Unix permission bits, npm resolves from the platform-specific Node distribution layout, and pnpm runs as JavaScript through the downloaded Node binary instead of relying on a shebang or host `PATH`.

**One tag publishes one complete desktop matrix.** A `desktop-v*` tag builds Windows x64/x86 NSIS installers, macOS Intel/Apple Silicon DMGs, and Linux x64 AppImage/deb packages. Each matrix job uploads an operating-system-and-architecture-qualified artifact; a dependent release job verifies the complete set before creating or updating one GitHub prerelease. These prerelease artifacts are unsigned and not notarized.

**One fish mark identifies every native surface.** The transparent black SVG path is shared with `FishLogo.tsx`; generated PNG, ICO, and ICNS resources feed the Tauri bundle, NSIS installer and uninstaller, configured splash window, and runtime-created main window.

## Alternatives considered

**Ship a complete offline dependency tree.** Rejected because the workspace dependency closure produces a very large installer and expensive filesystem operations. The trimmed source bundle keeps the release artifact small while retaining the exact built Harness application.

**Use Node, npm, or pnpm from the host.** Rejected because versions, installation paths, and availability differ across fresh Windows, macOS, and Linux systems. A private runtime gives first launch one controlled toolchain.

**Let each matrix job publish its own release assets.** Rejected because concurrent release creation can race and exposes a partial release while other platforms are still building. The final job publishes only after every required artifact exists.

**Publish only the locally verified Windows x64 installer.** Rejected because the desktop release contract includes Windows x86, both macOS architectures, and Linux x64, and runtime archive handling must match those binaries.

## Consequences

Installers remain compact, but first launch requires network access and can take several minutes while dependencies install. Runtime files and dependencies occupy application data rather than the installation directory. The release workflow spends build time on the full platform matrix and fails publication when any required package is absent. Users must explicitly approve unsigned prerelease binaries where operating-system security policy requires it.
