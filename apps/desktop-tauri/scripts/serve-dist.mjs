/**
 * Minimal static server for `tauri dev` splash UI.
 */
import { createServer } from 'node:http'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, extname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..', 'dist')
const port = Number(process.env.DSH_DESKTOP_DEV_PORT ?? 1430)

const mime = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
}

createServer((req, res) => {
  const url = req.url?.split('?')[0] ?? '/'
  const rel = url === '/' ? '/splash.html' : url
  const path = join(root, rel)
  if (!path.startsWith(root) || !existsSync(path)) {
    res.writeHead(404)
    res.end('not found')
    return
  }
  const body = readFileSync(path)
  res.writeHead(200, { 'Content-Type': mime[extname(path)] ?? 'application/octet-stream' })
  res.end(body)
}).listen(port, '127.0.0.1', () => {
  console.log(`serve-dist: http://127.0.0.1:${port}/splash.html`)
})
