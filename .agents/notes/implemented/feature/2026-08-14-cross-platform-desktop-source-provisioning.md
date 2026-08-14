# Agent Note: Cross-platform desktop source provisioning

Status: implemented

English | [中文](2026-08-14-cross-platform-desktop-source-provisioning.zh.md)

## Problem

The desktop shell needs small installers for users who do not have a Harness development environment. Shipping the complete workspace dependency tree makes installers large and slow to assemble, while assuming a system Node installation makes startup depend on unmanaged host tools. A release also needs independently attributable artifacts for each supported operating system and architecture without publishing a partial platform set.

## Decision

**Desktop installers carry a trimmed source tree and built application artifacts, but no `node_modules`.** First launch copies that immutable resource into application data, downloads the Node archive selected for the compiled operating system and architecture, installs pnpm through that Node runtime, and runs `pnpm install --prod --no-frozen-lockfile` with `CI` removed. Mirror endpoints remain configurable through `DSH_NODE_MIRROR` and `DSH_NPM_REGISTRY`.

**Each source bundle has an isolated writable tree.** The content hash selects `harness-versions/<bundle-hash>` so an update never deletes files held by an older running Host. Compatible Node and pnpm runtimes remain shared and are reused across source updates. The native shell admits one application instance and focuses its existing window on another launch.

**The downloaded runtime owns all provisioning commands.** Windows x64 and x86 use the official zip layouts; macOS x64/arm64 and Linux x64/arm64 use tar.gz layouts. Archive entries must remain below the expected versioned Node directory. Tar extraction preserves Unix permission bits, npm resolves from the platform-specific Node distribution layout, and pnpm runs as JavaScript through the downloaded Node binary instead of relying on a shebang or host `PATH`.

**One tag publishes one complete desktop matrix and one signed update manifest.** A `desktop-v*` tag builds Windows x64/x86 NSIS installers, macOS Intel/Apple Silicon DMGs, and Linux x64 AppImage/deb packages. Each matrix job signs its Tauri updater artifact and uploads operating-system-and-architecture-qualified files; a dependent release job verifies the complete set, creates or updates one GitHub prerelease, and replaces `latest.json` in the stable `desktop-updater` release channel. The updater public key is embedded in the application, while the private key and password exist only in release secrets and the maintainer's protected backup. Updater signatures authenticate downloads, but the executables remain without operating-system code signing or notarization.

**Release builds check and install updates before provisioning.** The official Tauri updater validates the stable manifest and artifact signature, installs a newer semantic version, and restarts the application. A failed update check or download is logged and startup continues with the installed version. Development builds skip network update checks.

**Windows installation closes the running application and invalidates stale shortcut icons.** The NSIS pre-install hook terminates `dsh-desktop.exe` with its child process tree before files are copied. The post-install hook preserves the user's desktop-shortcut choice, recreates an existing shortcut against a version-qualified standalone ICO resource, and notifies Explorer that icon associations changed.

**One fish mark identifies every native surface.** The transparent black SVG path is shared with `FishLogo.tsx`; generated PNG, ICO, and ICNS resources feed the Tauri bundle, NSIS installer and uninstaller, configured splash window, runtime-created main window, and Windows shortcut refresh.

## Alternatives considered

**Ship a complete offline dependency tree.** Rejected because the workspace dependency closure produces a very large installer and expensive filesystem operations. The trimmed source bundle keeps the release artifact small while retaining the exact built Harness application.

**Use Node, npm, or pnpm from the host.** Rejected because versions, installation paths, and availability differ across fresh Windows, macOS, and Linux systems. A private runtime gives first launch one controlled toolchain.

**Let each matrix job publish its own release assets.** Rejected because concurrent release creation can race and exposes a partial release while other platforms are still building. The final job publishes only after every required artifact exists.

**Publish only the locally verified Windows x64 installer.** Rejected because the desktop release contract includes Windows x86, both macOS architectures, and Linux x64, and runtime archive handling must match those binaries.

**Point installed applications at each version-specific prerelease.** Rejected because an endpoint embedded in one version cannot discover the next tag, while GitHub's latest-release redirect excludes prereleases. A constant release channel gives every installed version one durable manifest URL while versioned releases continue to own the downloadable artifacts.

**Implement update download and signature verification directly.** Rejected because Tauri's updater already defines platform artifact formats, semantic-version comparison, mandatory signature verification, Windows installer handoff, and application cleanup. A custom updater would duplicate security-sensitive behavior.

## Consequences

Installers remain compact, but first launch requires network access and can take several minutes while dependencies install. Runtime files and dependencies occupy application data rather than the installation directory. Source updates may temporarily retain an older bundle-specific tree while its Host still holds files; a later cleanup can remove inactive trees without blocking startup. Automatic updates require a newer semantic version, so replacing assets under the same tag bootstraps updater support only for users who reinstall that version once. Losing the updater private key or password prevents trusted updates for installed clients. The release workflow spends build time on the full platform matrix and fails publication when any required package or signature is absent. Manual Windows installation forcibly closes active application work, and users must explicitly approve binaries where operating-system security policy requires it.
