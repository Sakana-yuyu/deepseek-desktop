//! Native chrome locale: splash, tray, close dialog, and splash status lines.
//!
//! Follows the OS UI language (`zh*` → Chinese, otherwise English), matching
//! the NSIS installer. The embedded `dsh web` client keeps its own Settings
//! language. Unit tests default to Chinese so existing splash assertions stay
//! stable on English CI hosts.

use std::sync::OnceLock;

/// Shipped native-chrome locales, matching `@deepseek-ai/dsh-client-locale`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    /// Simplified Chinese copy.
    Zh,
    /// English copy.
    En,
}

/// User-visible native-chrome strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Msg {
    TrayShow,
    TrayCloseMin,
    TrayCloseExit,
    TrayCloseAsk,
    TrayEnvWindows,
    TrayEnvWsl,
    TrayUpdate,
    TrayRestart,
    TrayInstallCatalog,
    TrayQuit,
    TrayUpdateFailed,
    ToastCloseMin,
    ToastCloseExit,
    ToastCloseAsk,
    EnvRestart,
    EnvSaveFailed,
    CatalogInstalling,
    CatalogBusy,
    CatalogNotReady,
    CatalogFailed,
    CatalogRestarting,
    SplashPreparing,
    SplashBootFailed,
    StatusLocalRepo,
    StatusMatchingHome,
    StatusRuntimeReady,
    StatusScanToolchain,
    StatusExtractHarness,
    StatusReusedNode,
    StatusDownloadNode,
    StatusReusedPnpm,
    StatusInstallPnpm,
    StatusInstallDeps,
    StatusCheckProfile,
    StatusStartWeb,
    StatusDetectWsl,
    StatusPrepareWsl,
    StatusWritePath,
    StatusCheckUpdate,
    StatusDownloadUpdate,
    StatusHomeNone,
    StatusHomeMatched,
    StatusHomeRestored,
    StatusScanMatchedBoth,
    StatusScanMatchedNode,
    StatusScanMissingNode,
    StatusScanMissingBoth,
    WslMissing,
    Wsl1Only,
    WslDockerDefault,
    WslNamedMissing,
    WslNoneEligible,
    WslNoWindowsNode,
    WslErrHarness,
    WslErrNode,
    WslErrPnpm,
    WslErrOverlay,
    WslWaitForwarding,
    BootMissingBundle,
    BootRecoverIo,
    BootRecoverGeneric,
    BootRecoverFailed,
    PluginsDisabled,
    UpdaterDevSkip,
    UpdaterCurrent,
    NotifyTitle,
    NotifySessionDone,
    NotifyBody,
    ProfileInstalling,
    ProfileReady,
    ProfileInstallFailed,
    StatusDownloadLinuxNode,
}

static TEST_LOCALE: OnceLock<Locale> = OnceLock::new();
static PROCESS_LOCALE: OnceLock<Locale> = OnceLock::new();

/// Locale used for native chrome in this process.
pub fn current() -> Locale {
    if cfg!(test) {
        return *TEST_LOCALE.get_or_init(|| Locale::Zh);
    }
    *PROCESS_LOCALE.get_or_init(detect_os_locale)
}

/// Map a BCP 47 / `LANG` tag to a shipped chrome locale.
pub fn locale_from_tag(tag: &str) -> Locale {
    let primary = tag.split(['-', '_']).next().unwrap_or("");
    if primary.eq_ignore_ascii_case("zh") {
        Locale::Zh
    } else {
        Locale::En
    }
}

/// OS UI language → [`Locale`]. `zh*` is Chinese; every other tag is English.
pub fn detect_os_locale() -> Locale {
    locale_from_tag(&sys_locale::get_locale().unwrap_or_default())
}

/// Look up a chrome string in [`current`].
pub fn t(msg: Msg) -> &'static str {
    match current() {
        Locale::Zh => zh(msg),
        Locale::En => en(msg),
    }
}

/// Substitute every `{0}` in a chrome template.
pub fn tf(msg: Msg, arg: &str) -> String {
    t(msg).replace("{0}", arg)
}

/// Substitute every `{0}` then `{1}` in a chrome template.
pub fn tf2(msg: Msg, first: &str, second: &str) -> String {
    t(msg).replace("{0}", first).replace("{1}", second)
}

fn zh(msg: Msg) -> &'static str {
    match msg {
        Msg::TrayShow => "显示窗口",
        Msg::TrayCloseMin => "关闭时最小化到托盘",
        Msg::TrayCloseExit => "关闭时退出程序",
        Msg::TrayCloseAsk => "下次关闭时再询问",
        Msg::TrayEnvWindows => "运行环境：Windows",
        Msg::TrayEnvWsl => "运行环境：WSL（需重启）",
        Msg::TrayUpdate => "检查更新",
        Msg::TrayRestart => "重启",
        Msg::TrayInstallCatalog => "安装插件库",
        Msg::TrayQuit => "退出",
        Msg::TrayUpdateFailed => "检查更新失败",
        Msg::ToastCloseMin => "关闭窗口将最小化到托盘",
        Msg::ToastCloseExit => "关闭窗口将退出程序",
        Msg::ToastCloseAsk => "下次关闭窗口时会再询问",
        Msg::EnvRestart => "运行环境将在重启后生效",
        Msg::EnvSaveFailed => "保存运行环境失败",
        Msg::CatalogInstalling => "正在安装插件库…",
        Msg::CatalogBusy => "插件库正在安装",
        Msg::CatalogNotReady => "请等客户端启动完成后再安装插件库",
        Msg::CatalogFailed => "安装插件库失败",
        Msg::CatalogRestarting => "插件库已安装，正在重启…",
        Msg::SplashPreparing => "正在准备运行环境…",
        Msg::SplashBootFailed => "启动失败",
        Msg::StatusLocalRepo => "使用本地仓库…",
        Msg::StatusMatchingHome => "正在匹配已有对话与密钥…",
        Msg::StatusRuntimeReady => "运行环境已就绪",
        Msg::StatusScanToolchain => "正在扫描本机 Node / pnpm…",
        Msg::StatusExtractHarness => "正在释放 harness 源码…",
        Msg::StatusReusedNode => "已复用本机 Node，跳过下载",
        Msg::StatusDownloadNode => "正在从镜像下载 Node {0}…",
        Msg::StatusReusedPnpm => "已复用本机 pnpm",
        Msg::StatusInstallPnpm => "正在安装 pnpm {0}…",
        Msg::StatusInstallDeps => "正在从镜像安装依赖 (pnpm install --prod --no-frozen-lockfile)…",
        Msg::StatusCheckProfile => "正在检查 profile 依赖…",
        Msg::StatusStartWeb => "正在启动 Web 界面…",
        Msg::StatusDetectWsl => "正在检测 WSL…",
        Msg::StatusPrepareWsl => "正在准备 WSL 运行环境…",
        Msg::StatusWritePath => "正在写入 dsh 命令并加入 PATH…",
        Msg::StatusCheckUpdate => "正在检查桌面更新…",
        Msg::StatusDownloadUpdate => "正在下载桌面更新 {0}…",
        Msg::StatusHomeNone => "未发现已有对话，将使用桌面端主目录",
        Msg::StatusHomeMatched => "已匹配已有主目录 {0}",
        Msg::StatusHomeRestored => "已恢复 {0} 项历史数据到 {1}",
        Msg::StatusScanMatchedBoth => "已匹配本机 Node {0}，跳过运行时下载",
        Msg::StatusScanMatchedNode => "已匹配本机 Node {0}，将仅安装 pnpm",
        Msg::StatusScanMissingNode => "未找到兼容 Node，将下载运行时并复用本机 pnpm",
        Msg::StatusScanMissingBoth => "未找到兼容 Node / pnpm，将从镜像下载",
        Msg::WslMissing => "未检测到 WSL。请安装 WSL2 后再将运行环境设为 WSL。",
        Msg::Wsl1Only => "当前发行版是 WSL1。请执行 wsl --set-version <发行版> 2。",
        Msg::WslDockerDefault => {
            "默认 WSL 发行版是 Docker。请执行 wsl --set-default <Ubuntu 发行版名>。"
        }
        Msg::WslNamedMissing => {
            "找不到 WSL 发行版 {0}。请检查 desktop-settings.json 的 wslDistro。"
        }
        Msg::WslNoneEligible => "没有可用的 WSL2 发行版（已跳过 docker-desktop）。",
        Msg::WslNoWindowsNode => "禁止在 WSL 中执行 Windows node.exe",
        Msg::WslErrHarness => "无法在 WSL 中安装 harness 树",
        Msg::WslErrNode => "无法在 WSL 中安装 Node",
        Msg::WslErrPnpm => "无法在 WSL 中执行 pnpm install",
        Msg::WslErrOverlay => "无法写入 WSL overlay",
        Msg::WslWaitForwarding => "请检查 WSL 的 localhost 转发（localhostForwarding）。",
        Msg::BootMissingBundle => "安装包内缺少 harness 源码资源；请重新构建 desktop-tauri",
        Msg::BootRecoverIo => "预配遇到占用或权限问题，改用已有运行时…",
        Msg::BootRecoverGeneric => "预配未完成，改用已有运行时…",
        Msg::BootRecoverFailed => "启动遇到占用或权限问题，未能找到可用运行时。详见 boot.log。",
        Msg::PluginsDisabled => {
            "以下插件已损坏，本次启动已自动禁用：{0}。修复或更新插件后重启即可恢复。"
        }
        Msg::UpdaterDevSkip => "开发构建不检查桌面更新",
        Msg::UpdaterCurrent => "当前已是最新版本",
        Msg::NotifyTitle => "任务完成",
        Msg::NotifySessionDone => "会话 {0} 已完成",
        Msg::NotifyBody => "DeepSeek Harness 已完成本轮任务",
        Msg::ProfileInstalling => "正在安装 profile {0} 依赖…",
        Msg::ProfileReady => "profile {0} 依赖已就绪",
        Msg::ProfileInstallFailed => {
            "profile {0} 依赖安装失败: {1}\n请检查网络后重试，或手动运行 dsh plugin --profile {0} install"
        }
        Msg::StatusDownloadLinuxNode => "正在下载 Linux Node {0}…",
    }
}

fn en(msg: Msg) -> &'static str {
    match msg {
        Msg::TrayShow => "Show window",
        Msg::TrayCloseMin => "Minimize to tray on close",
        Msg::TrayCloseExit => "Quit on close",
        Msg::TrayCloseAsk => "Ask next time on close",
        Msg::TrayEnvWindows => "Runtime: Windows",
        Msg::TrayEnvWsl => "Runtime: WSL (restart required)",
        Msg::TrayUpdate => "Check for updates",
        Msg::TrayRestart => "Restart",
        Msg::TrayInstallCatalog => "Install plugin catalog",
        Msg::TrayQuit => "Quit",
        Msg::TrayUpdateFailed => "Update check failed",
        Msg::ToastCloseMin => "Closing the window will minimize to the tray",
        Msg::ToastCloseExit => "Closing the window will quit the app",
        Msg::ToastCloseAsk => "The next close will ask again",
        Msg::EnvRestart => "Runtime takes effect after restart",
        Msg::EnvSaveFailed => "Could not save runtime",
        Msg::CatalogInstalling => "Installing the plugin catalog…",
        Msg::CatalogBusy => "The plugin catalog is already installing",
        Msg::CatalogNotReady => "Wait until the client has started, then install the plugin catalog",
        Msg::CatalogFailed => "Plugin catalog install failed",
        Msg::CatalogRestarting => "Plugin catalog installed; restarting…",
        Msg::SplashPreparing => "Preparing the runtime…",
        Msg::SplashBootFailed => "Startup failed",
        Msg::StatusLocalRepo => "Using the local repository…",
        Msg::StatusMatchingHome => "Matching existing sessions and credentials…",
        Msg::StatusRuntimeReady => "Runtime is ready",
        Msg::StatusScanToolchain => "Scanning this machine for Node / pnpm…",
        Msg::StatusExtractHarness => "Unpacking harness source…",
        Msg::StatusReusedNode => "Reusing this machine's Node; skip download",
        Msg::StatusDownloadNode => "Downloading Node {0} from the mirror…",
        Msg::StatusReusedPnpm => "Reusing this machine's pnpm",
        Msg::StatusInstallPnpm => "Installing pnpm {0}…",
        Msg::StatusInstallDeps => {
            "Installing dependencies from the mirror (pnpm install --prod --no-frozen-lockfile)…"
        }
        Msg::StatusCheckProfile => "Checking profile dependencies…",
        Msg::StatusStartWeb => "Starting the web UI…",
        Msg::StatusDetectWsl => "Detecting WSL…",
        Msg::StatusPrepareWsl => "Preparing the WSL runtime…",
        Msg::StatusWritePath => "Writing the dsh command and updating PATH…",
        Msg::StatusCheckUpdate => "Checking for desktop updates…",
        Msg::StatusDownloadUpdate => "Downloading desktop update {0}…",
        Msg::StatusHomeNone => "No existing sessions found; using the desktop home",
        Msg::StatusHomeMatched => "Matched existing home {0}",
        Msg::StatusHomeRestored => "Restored {0} history items into {1}",
        Msg::StatusScanMatchedBoth => "Matched this machine's Node {0}; skip runtime download",
        Msg::StatusScanMatchedNode => "Matched this machine's Node {0}; will install pnpm only",
        Msg::StatusScanMissingNode => {
            "No compatible Node found; will download the runtime and reuse this machine's pnpm"
        }
        Msg::StatusScanMissingBoth => "No compatible Node / pnpm found; will download from the mirror",
        Msg::WslMissing => "WSL was not detected. Install WSL2, then set the runtime to WSL.",
        Msg::Wsl1Only => "This distro is WSL1. Run wsl --set-version <distro> 2.",
        Msg::WslDockerDefault => {
            "The default WSL distro is Docker. Run wsl --set-default <Ubuntu distro name>."
        }
        Msg::WslNamedMissing => {
            "WSL distro {0} was not found. Check wslDistro in desktop-settings.json."
        }
        Msg::WslNoneEligible => "No usable WSL2 distro (docker-desktop skipped).",
        Msg::WslNoWindowsNode => "Do not run Windows node.exe inside WSL",
        Msg::WslErrHarness => "Could not install the harness tree in WSL",
        Msg::WslErrNode => "Could not install Node in WSL",
        Msg::WslErrPnpm => "Could not run pnpm install in WSL",
        Msg::WslErrOverlay => "Could not write the WSL overlay",
        Msg::WslWaitForwarding => "Check WSL localhost forwarding (localhostForwarding).",
        Msg::BootMissingBundle => {
            "The installer is missing harness source; rebuild desktop-tauri"
        }
        Msg::BootRecoverIo => {
            "Provisioning hit a lock or permission issue; using an existing runtime…"
        }
        Msg::BootRecoverGeneric => "Provisioning did not finish; using an existing runtime…",
        Msg::BootRecoverFailed => {
            "Startup hit a lock or permission issue and no runtime was found. See boot.log."
        }
        Msg::PluginsDisabled => {
            "These plugins failed to load and were disabled for this launch: {0}. Restart after you repair or update them."
        }
        Msg::UpdaterDevSkip => "Dev builds do not check for desktop updates",
        Msg::UpdaterCurrent => "You are already on the latest version",
        Msg::NotifyTitle => "Task complete",
        Msg::NotifySessionDone => "Session {0} finished",
        Msg::NotifyBody => "DeepSeek Harness finished this turn",
        Msg::ProfileInstalling => "Installing profile {0} dependencies…",
        Msg::ProfileReady => "Profile {0} dependencies are ready",
        Msg::ProfileInstallFailed => {
            "Profile {0} dependency install failed: {1}\nCheck the network and retry, or run: dsh plugin --profile {0} install"
        }
        Msg::StatusDownloadLinuxNode => "Downloading Linux Node {0}…",
    }
}

#[cfg(test)]
mod tests {
    use super::{locale_from_tag, t, tf, Locale, Msg};

    #[test]
    fn tests_default_to_chinese() {
        assert_eq!(t(Msg::TrayShow), "显示窗口");
        assert_eq!(t(Msg::TrayRestart), "重启");
        assert_eq!(t(Msg::TrayInstallCatalog), "安装插件库");
        assert_eq!(t(Msg::SplashPreparing), "正在准备运行环境…");
        assert_eq!(t(Msg::SplashBootFailed), "启动失败");
        assert!(t(Msg::WslMissing).contains("WSL2"));
    }

    #[test]
    fn interpolates_named_distro() {
        assert!(tf(Msg::WslNamedMissing, "Debian").contains("Debian"));
    }

    #[test]
    fn os_tag_maps_zh_prefix() {
        assert_eq!(locale_from_tag("zh-CN"), Locale::Zh);
        assert_eq!(locale_from_tag("zh_TW"), Locale::Zh);
        assert_eq!(locale_from_tag("en-US"), Locale::En);
        assert_eq!(locale_from_tag("fr-FR"), Locale::En);
        assert_eq!(locale_from_tag(""), Locale::En);
    }
}
