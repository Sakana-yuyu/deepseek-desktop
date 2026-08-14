import assert from 'node:assert/strict'
import { afterEach, test } from 'node:test'
import { apply } from './index.mjs'

const originalFetch = globalThis.fetch
const originalUrl = process.env.DSH_DESKTOP_NOTIFY_URL

afterEach(() => {
  globalThis.fetch = originalFetch
  if (originalUrl === undefined) {
    delete process.env.DSH_DESKTOP_NOTIFY_URL
  } else {
    process.env.DSH_DESKTOP_NOTIFY_URL = originalUrl
  }
})

function loadPlugin() {
  const listeners = []
  apply({
    on(name, fn) {
      listeners.push([name, fn])
    },
  })
  return listeners
}

test('does not subscribe when the notify URL is absent', () => {
  delete process.env.DSH_DESKTOP_NOTIFY_URL
  assert.equal(loadPlugin().length, 0)
})

test('posts only a completed turn/end to the shell', async () => {
  process.env.DSH_DESKTOP_NOTIFY_URL = 'http://127.0.0.1:9/notify'
  const calls = []
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init })
    return new Response(null, { status: 204 })
  }

  const listeners = loadPlugin()
  assert.equal(listeners.length, 1)
  assert.equal(listeners[0][0], 'session/event')

  listeners[0][1]({ id: 'sess-1' }, { type: 'turn/start' })
  listeners[0][1](
    { id: 'sess-1' },
    { type: 'turn/end', data: { reason: { kind: 'aborted' } } },
  )
  listeners[0][1](
    { id: 'sess-1' },
    { type: 'turn/end', data: { reason: { kind: 'completed' } } },
  )
  await new Promise((resolve) => setTimeout(resolve, 0))

  assert.equal(calls.length, 1)
  assert.equal(calls[0].url, 'http://127.0.0.1:9/notify')
  assert.equal(JSON.parse(calls[0].init.body).sessionId, 'sess-1')
})
