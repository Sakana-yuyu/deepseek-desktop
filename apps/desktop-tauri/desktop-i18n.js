/**
 * Native chrome dictionaries for splash.html and shell.html.
 * Locale follows the OS language (zh* → zh, otherwise en).
 */
window.DSH_I18N = (function () {
  const zh = {
    'splash.preparing': '正在准备运行环境…',
    'splash.failed': '启动失败',
    'shell.closeTitle': '关闭窗口',
    'shell.closeDesc': '下次将记住这个选择，可在托盘菜单里改回。',
    'shell.exit': '退出程序',
    'shell.minimizeTray': '最小化到托盘',
    'shell.cancel': '取消',
    'shell.min': '最小化',
    'shell.max': '最大化',
    'shell.close': '关闭',
  }
  const en = {
    'splash.preparing': 'Preparing the runtime…',
    'splash.failed': 'Startup failed',
    'shell.closeTitle': 'Close window',
    'shell.closeDesc': 'This choice is remembered; change it later from the tray.',
    'shell.exit': 'Quit',
    'shell.minimizeTray': 'Minimize to tray',
    'shell.cancel': 'Cancel',
    'shell.min': 'Minimize',
    'shell.max': 'Maximize',
    'shell.close': 'Close',
  }

  function detect() {
    if (window.__DSH_LOCALE__ === 'zh' || window.__DSH_LOCALE__ === 'en') {
      return window.__DSH_LOCALE__
    }
    const tag = String(navigator.language || navigator.userLanguage || '')
    const primary = tag.split(/[-_]/)[0]
    return primary.toLowerCase() === 'zh' ? 'zh' : 'en'
  }

  function dict() {
    return detect() === 'zh' ? zh : en
  }

  function t(key) {
    const table = dict()
    return table[key] || zh[key] || key
  }

  function apply(root) {
    const locale = detect()
    document.documentElement.lang = locale === 'zh' ? 'zh-CN' : 'en'
    const scope = root || document
    scope.querySelectorAll('[data-i18n]').forEach((el) => {
      const key = el.getAttribute('data-i18n')
      if (key) el.textContent = t(key)
    })
    scope.querySelectorAll('[data-i18n-aria]').forEach((el) => {
      const key = el.getAttribute('data-i18n-aria')
      if (key) el.setAttribute('aria-label', t(key))
    })
  }

  return { detect, t, apply }
})()
