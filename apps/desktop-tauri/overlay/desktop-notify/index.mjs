/**
 * Desktop overlay plugin: notify the Tauri shell when a turn completes.
 *
 * Loaded through `dsh web --patch` from the desktop shell. It does not live
 * in `packages/` and is not part of the upstream Harness tree.
 */

export const name = 'dsh-desktop-notify'

/**
 * Subscribe to session events and POST completed turns to the native shell.
 * @param {import('@deepseek-ai/cordis').Context} ctx
 */
export function apply(ctx) {
  const notifyUrl = process.env.DSH_DESKTOP_NOTIFY_URL
  if (!notifyUrl) {
    return
  }

  ctx.on('session/event', (session, event) => {
    if (event?.type !== 'turn/end') {
      return
    }
    if (event.data?.reason?.kind !== 'completed') {
      return
    }

    const payload = JSON.stringify({
      title: '任务完成',
      body: 'DeepSeek Harness 已完成本轮任务',
      sessionId: session?.id ?? '',
      reason: 'completed',
    })

    fetch(notifyUrl, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: payload,
    }).catch(() => {
      // The shell may have exited; a missed toast must not fail the Host.
    })
  })
}
