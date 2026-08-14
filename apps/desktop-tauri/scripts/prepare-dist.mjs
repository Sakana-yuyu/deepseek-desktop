/**
 * Prepare Tauri frontend dist and bundled harness source tree.
 */
import { cpSync, existsSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const dist = join(root, 'dist')
mkdirSync(dist, { recursive: true })
cpSync(join(root, 'splash.html'), join(dist, 'splash.html'))
cpSync(join(root, 'shell.html'), join(dist, 'shell.html'))
cpSync(join(root, 'app-icon.png'), join(dist, 'app-icon.png'))

const bundleScript = join(root, 'scripts', 'bundle-harness-source.mjs')
const result = spawnSync(process.execPath, [bundleScript], { stdio: 'inherit', cwd: root })
if (result.status !== 0) {
  process.exit(result.status ?? 1)
}

if (!existsSync(join(root, 'bundled', 'harness', '.bundle-manifest.json'))) {
  throw new Error('bundled harness manifest missing after bundle-harness-source.mjs')
}
