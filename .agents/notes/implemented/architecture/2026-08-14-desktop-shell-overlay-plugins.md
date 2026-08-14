# Agent Note: Desktop shell chrome and overlay plugins

Status: implemented

English | [中文](2026-08-14-desktop-shell-overlay-plugins.zh.md)

## Problem

The desktop fork must add window chrome, a tray, signed updates, and task-complete alerts without editing upstream Harness packages. Later syncs should pull `packages/`, `apps/cli`, and `apps/web` unchanged. Features that must observe Host session events cannot live only in the WebView, because that would require changing the shipped web client.

## Decision

**Native chrome stays in `apps/desktop-tauri`.** The main window is frameless. A local `shell.html` title bar hosts the fish mark, the title, and min/max/close. The splash is a compact transparent window with a borderless radial frost (opaque center, fading edges) using the dark tokens from `design-platform.css` (official fish mark, DeepSeek wordmark, HARNESS badge, and a progress bar). The first close opens an in-window modal in `shell.html` that matches the web client's light `Modal` (mask, r24 card, capsule outline / primary buttons) and writes the answer to `%APPDATA%/DeepSeek Harness/desktop-settings.json` (`closeAction`). Later closes honor that file. The tray can switch 关闭时最小化到托盘 / 关闭时退出程序 / 下次关闭时再询问 without quitting. Hiding the last window does not exit: `ExitRequested` is cancelled until tray 退出 or a saved Exit close. Quitting (tray 退出, close-action Exit, or updater restart) stops the Host Node process tree before `app.exit`; Windows assigns that tree to a job with `KILL_ON_JOB_CLOSE` so grandchildren die even when destructors do not run. A later launch reaps a stale `host.pid` when the recorded Node image still matches. Clicking the tray shows the window. Single-instance still focuses the existing window. Windows places those buttons on the right; macOS places them on the left; Linux parses the window-manager button layout (`gsettings` / XFCE, overridable with `DSH_DESKTOP_BUTTON_LAYOUT`) and may split buttons across both sides.

**Host collaboration is an overlay plugin, not a package edit.** The shell copies `overlay/desktop-notify/index.mjs` into `$DSH_HOME/desktop-overlay`, writes a `--patch` list whose plugin `name` is a `file://` URL, and starts `dsh web --patch <that file>`. A Windows drive path such as `C:/...` is not a valid ESM specifier — Node reads `C:` as a URL scheme — so the overlay must emit `file:///C:/...` (spaces percent-encoded). The plugin listens for `session/event` `turn/end` with `reason.kind === 'completed'` and POSTs to a loopback notify URL supplied as `DSH_DESKTOP_NOTIFY_URL`. The Rust listener shows a system toast and plays `sounds/complete.wav` only when the main window is unfocused.

**Updates remain the signed Tauri updater.** The signed check runs after the main window opens so a failed or slow network does not hold the splash. The tray "检查更新" item runs the same signed check on demand.

This extends [cross-platform desktop source provisioning](../feature/2026-08-14-cross-platform-desktop-source-provisioning.md) without moving desktop behavior into `packages/`.

## Alternatives considered

**Patch `apps/web` or a `packages/*` plugin.** Rejected because every upstream sync would re-apply or lose the desktop behavior. The overlay uses the documented `--patch` layer instead.

**Write `$DSH_HOME/cordis.patch.yml` directly.** Rejected because that file is the user's home-level patch layer. A generated `--patch` file leaves the home file for the user.

**Inject a title bar into the React DOM.** Rejected because it couples chrome to web client markup and still cannot own tray, update, or OS notifications.

**Always quit on the title-bar close button.** Rejected because a coding session should survive an accidental close; the first close asks, then the saved preference and the tray own process lifetime.

**Always hide on close with no prompt.** Rejected because some users want close to exit, and a missing tray made hide look like a crash.

## Consequences

Upstream framework trees stay free of desktop-only rows. A missing overlay file fails Host startup loud. Users who already have a home `cordis.patch.yml` keep it. Focused-window turns do not toast or chime. Linux hosts without a readable button layout get Windows-style right-side controls. Screenshot assets in `apps/desktop-tauri/screenshots/` are illustrative of the shell, not recorded from a live session.
