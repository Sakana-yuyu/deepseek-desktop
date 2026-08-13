# DeepSeek Harness Desktop (Tauri)

English | [中文](README.zh.md)

Rust/WebView2 shell over the existing `dsh web` UI. The installer ships **harness source** (no `node_modules`); first run pulls **build tools only** from mirrors, then runs `pnpm install --prod` against the bundled tree.

Current desktop prerelease: **0.1.0-rc.5**.

## Architecture

| Layer | What ships | First run |
|---|---|---|
| **Installer** | Tauri binary + splash + trimmed monorepo slice (`bundled/harness/`) | — |
| **Build env** | — | Node (npmmirror), pnpm (via npm + npmmirror registry) |
| **Dependencies** | — | `pnpm install --prod --no-frozen-lockfile` in the platform application-data directory (trimmed bundle vs lockfile; `CI` unset so pnpm does not force frozen install) |
| **Host** | — | `node apps/cli/lib/bin.js web --host 127.0.0.1` |
| **UI** | — | WebView2 → `http://127.0.0.1:17890` (existing React web client) |

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

- `harness/` — seeded source + `node_modules` after first `pnpm install`
- `runtime/` — Node, pnpm-global, manifest
- `dsh-home/` — session data (`DSH_HOME`)
- `cache/` — downloaded Node zip or tarball

The provisioner selects Node for Windows x64/x86, macOS x64/arm64, and Linux x64/arm64. Zip and tar.gz extraction reject entries outside the expected Node archive root. Unix archives retain executable permissions, and pnpm runs through the downloaded Node binary so first launch does not depend on a system Node installation.

## Build

From the repo root (needs built CLI + web dist):

```powershell
pnpm run build
cd apps/desktop-tauri
pnpm install
pnpm run build:win
```

Installer output: `src-tauri/target/release/bundle/nsis/DeepSeek Harness_0.1.0-rc.5_x64-setup.exe`

The NSIS installer bundles **English**, **Simplified Chinese**, and **Traditional Chinese**. Language follows the OS locale automatically (no language picker); if the locale is unsupported, English is used.

## Release

Pushing a `desktop-v*` tag runs [the desktop release workflow](../../.github/workflows/desktop-release.yml). It builds Windows x64/x86 NSIS installers, macOS Intel/Apple Silicon DMGs, and Linux x64 AppImage/deb packages, then publishes one prerelease after every matrix job succeeds. A manual dispatch rebuilds an existing tag.

Release asset names include the operating system and architecture. The cloud artifacts are currently unsigned and not notarized, so Windows SmartScreen, macOS Gatekeeper, or Linux desktop security prompts may require explicit approval.

All Windows, macOS, and Linux icons are generated from `app-icon.svg`, which carries the same background-free black fish path as `packages/client/ui-primitives/src/FishLogo.tsx`. The Tauri bundle, NSIS installer/uninstaller, splash window, main title bar, taskbar, Dock, and Linux desktop entry use that icon set.

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

**Installed app:** run the NSIS installer; first launch shows splash while Node/pnpm/deps install, then opens the web UI.

## Scripts

| Script | Purpose |
|---|---|
| `scripts/bundle-harness-source.mjs` | Trim + copy monorepo slice → `bundled/harness/` |
| `scripts/prepare-dist.mjs` | Splash dist + bundle (Tauri `beforeBuildCommand`) |
| `scripts/serve-dist.mjs` | Static server for `tauri dev` splash |
