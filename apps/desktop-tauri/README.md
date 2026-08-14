# DeepSeek Harness Desktop (Tauri)

English | [中文](README.zh.md)

Rust/WebView2 shell over the existing `dsh web` UI. The installer ships **harness source** (no `node_modules`); first run scans the host for a compatible Node / pnpm and an existing `~/.dsh` home, downloads **build tools only** when the scan finds none, then runs `pnpm install --prod` against the bundled tree.

Current desktop prerelease: **0.1.0-rc.5-0.1**.

## Architecture

| Layer | What ships | First run |
|---|---|---|
| **Installer** | Tauri binary + splash + trimmed monorepo slice (`bundled/harness/`) | — |
| **Build env** | — | Reuse host Node 22.19+ or 24+ and pnpm when present; otherwise Node (npmmirror) and pnpm (via npm + npmmirror registry) |
| **Dependencies** | — | `pnpm install --prod --no-frozen-lockfile` in the platform application-data directory (trimmed bundle vs lockfile; `CI` unset so pnpm does not force frozen install) |
| **Host** | — | `node apps/cli/lib/bin.js web --host 127.0.0.1` |
| **UI** | Local `shell.html` title bar | Frameless window embeds `dsh web`; Windows controls on the right, macOS on the left, Linux from the window-manager button layout |
| **Tray** | Native tray icon | Close hides to tray; menu shows the window, checks for updates, or quits |
| **Notify** | Overlay plugin + localhost POST | `turn/end` with `completed` shows a toast and plays `sounds/complete.wav` when the window is unfocused |
| **Updates** | Embedded updater public key | Check the stable GitHub update manifest, verify the downloaded artifact signature, install, and restart |

Bundled tree includes: `apps/cli` (with built `lib/`), `apps/web` (with `dist/`), `packages/*/*` (excluding examples and test-support), `native/landlock-run`, `vendor/*`, `patches/`, lockfile — **not** the old 900k-junction offline runtime. Workspace `devDependencies` are stripped at bundle time so `--prod` install does not require demo packages.

### Mirrors (override via env)

| Variable | Default |
|---|---|
| `DSH_NODE_MIRROR` | `https://npmmirror.com/mirrors/node` |
| `DSH_NPM_REGISTRY` | `https://registry.npmmirror.com` |

### Dev vs production

| Mode | Env | Behavior |
|---|---|---|
| **Local dev** | `DSH_DESKTOP_LAUNCH=local` | Use monorepo checkout + PATH `node`/`pnpm`; skip mirror fetch |
| **Production** | (default) | Copy `harness-source` from installer → app data → mirror install → boot |

Writable paths live under the platform application-data directory (`%APPDATA%\DeepSeek Harness` on Windows, `~/Library/Application Support/ai.deepseek.dsh-desktop` on macOS, and the platform data directory returned by Tauri on Linux):

- `harness-versions/<bundle-hash>/` — bundle-specific source + `node_modules` after first `pnpm install`
- `runtime/` — Node, pnpm-global, manifest
- `dsh-home/` — fallback session data when no existing Harness home is found
- `bin/` — `dsh` shims written onto the Host PATH and, when missing, the user Path
- `cache/` — downloaded Node zip or tarball

First launch scans `PATH` and well-known install locations for Node `^22.19 || >=24` and a usable pnpm before any mirror fetch. It then adopts `$DSH_HOME` or `~/.dsh` when that directory already holds sessions, credentials, `.env`, profiles, or settings, and copies missing files from the isolated `dsh-home/` into the selected home. It writes `dsh` shims and prepends the selected Node / pnpm directories (plus Git `cmd`/`bin` when `git` or `bash` is missing) onto the Host PATH so `dsh plugin`, MCP `npx`, and agent `bash`/`git` lookups resolve. The user Path receives the shim directory, and the Node or pnpm directory only when that command is still absent. The provisioner still downloads Node for Windows x64/x86, macOS x64/arm64, and Linux x64/arm64 when the scan finds none. Zip and tar.gz extraction reject entries outside the expected Node archive root. Unix archives retain executable permissions. A privately installed pnpm runs through the selected Node binary; a host pnpm is invoked directly. Bundle-specific harness directories let an update provision new source without deleting files used by an older running Host; compatible Node and pnpm runtimes are reused across source updates. The native shell permits one application instance and focuses the existing window on repeated launches. Release builds check for an update before provisioning; update-network or manifest failures are logged and do not block startup. Window chrome, tray, updater, toast, and completion sound stay in this Rust crate. Host collaboration is an overlay plugin copied into `$DSH_HOME/desktop-overlay` and loaded with `dsh web --patch`; `packages/` is not modified.

## Build

From the repo root (needs built CLI + web dist):

```powershell
pnpm run build
cd apps/desktop-tauri
pnpm install
$env:TAURI_SIGNING_PRIVATE_KEY=(Get-Content "$HOME\.tauri\deepseek-desktop-updater.key" -Raw)
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD=(Get-Content "$HOME\.tauri\deepseek-desktop-updater.key.password" -Raw)
pnpm run build:win
```

Installer output: `src-tauri/target/release/bundle/nsis/DeepSeek Harness_0.1.0-rc.5-0.1_x64-setup.exe`

The NSIS installer bundles **English**, **Simplified Chinese**, and **Traditional Chinese**. Language follows the OS locale automatically (no language picker); if the locale is unsupported, English is used. Before copying files, the installer silently closes `dsh-desktop.exe` and its child process tree. After installation, it recreates an existing desktop shortcut with the versioned standalone ICO resource and notifies Explorer to invalidate stale icon cache entries.

## Release

Pushing a `desktop-v*` tag runs [the desktop release workflow](../../.github/workflows/desktop-release.yml). It builds Windows x64/x86 NSIS installers, macOS Intel/Apple Silicon DMGs, and Linux x64 AppImage/deb packages, then publishes one prerelease after every matrix job succeeds. The workflow signs each updater artifact, generates the Tauri `latest.json`, and replaces the manifest in the stable `desktop-updater` release channel. A manual dispatch rebuilds an existing tag.

Release asset names include the operating system and architecture. Updater signatures authenticate artifacts to installed applications, but the executables are not operating-system code-signed or notarized, so Windows SmartScreen, macOS Gatekeeper, or Linux desktop security prompts may require explicit approval.

All Windows, macOS, and Linux icons are generated from `app-icon.svg`, which carries the same background-free black fish path as `packages/client/ui-primitives/src/FishLogo.tsx`. The Tauri bundle, NSIS installer/uninstaller, splash window, main title bar, taskbar, Dock, and Linux desktop entry use that icon set. Windows installation also includes a version-qualified ICO file so shortcut icon lookup does not reuse an older executable-path cache key.

Bundle only (no Tauri):

```powershell
node scripts/bundle-harness-source.mjs
```

## Run

**Dev (monorepo checkout):**

```powershell
# repo root: pnpm run build  (once)
cd apps/desktop-tauri
$env:DSH_DESKTOP_LAUNCH='local'
pnpm run dev
```

**Installed app:** run the NSIS installer; first launch shows splash while it scans the host, matches an existing `~/.dsh` home, installs missing tools or deps, then opens the web UI.

## Scripts

| Script | Purpose |
|---|---|
| `scripts/bundle-harness-source.mjs` | Trim + copy monorepo slice → `bundled/harness/` |
| `scripts/prepare-dist.mjs` | Splash dist + bundle (Tauri `beforeBuildCommand`) |
| `scripts/serve-dist.mjs` | Static server for `tauri dev` splash |
| `overlay/desktop-notify/` | Cordis overlay: POST completed turns to the native notify port |
