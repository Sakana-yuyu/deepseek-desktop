/**
 * Bundle a trimmed harness monorepo slice for the Tauri installer.
 *
 * Ships source + pre-built lib/dist artifacts, never node_modules.
 * First-run provisioning runs `pnpm install --prod` against this tree.
 */
import { createHash } from 'node:crypto'
import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { execSync } from 'node:child_process'
import { dirname, join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const desktopRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const repoRoot = join(desktopRoot, '..', '..')
const outRoot = join(desktopRoot, 'bundled', 'harness')

const skipDirNames = new Set([
  'node_modules', '.git', '.turbo', 'coverage', 'release', '.stage', '.cache',
  'tests', 'test', '__tests__', 'dist-test',
])

const skipPackageGroups = new Set(['examples', 'test-support'])

const skipFileSuffixes = ['.spec.ts', '.e2e.ts', '.snapshot.ts']

/** @param {string} sourceRoot @param {string} source */
function shouldCopyEntry(sourceRoot, source) {
  const rel = relative(sourceRoot, source)
  if (rel === '') return true
  const parts = rel.split(sep)
  if (parts.some(part => skipDirNames.has(part))) return false
  const base = parts[parts.length - 1]
  if (skipFileSuffixes.some(suffix => base.endsWith(suffix))) return false
  if (base.startsWith('README') && parts.length > 2) return false
  return true
}

/** @param {string} sourceRoot @param {string} current @param {import('node:crypto').Hash} hasher @param {string} relPrefix */
function hashSourceWalk(sourceRoot, current, hasher, relPrefix) {
  for (const entry of readdirSync(current, { withFileTypes: true })) {
    const path = join(current, entry.name)
    if (!shouldCopyEntry(sourceRoot, path)) continue
    if (entry.isDirectory()) {
      const dirRel = relPrefix ? `${relPrefix}/${entry.name}`.replaceAll('\\', '/') : entry.name.replaceAll('\\', '/')
      hashSourceWalk(sourceRoot, path, hasher, dirRel)
      continue
    }
    if (!entry.isFile()) continue
    const rel = relPrefix ? `${relPrefix}/${entry.name}`.replaceAll('\\', '/') : entry.name.replaceAll('\\', '/')
    hasher.update(rel)
    if (entry.name === 'package.json') {
      const pkg = JSON.parse(readFileSync(path, 'utf8'))
      delete pkg.devDependencies
      hasher.update(JSON.stringify(pkg, null, 2))
    }
    else {
      hasher.update(readFileSync(path))
    }
  }
}

/** Hash the same source slices we copy into the installer bundle. */
function hashBundledContent(trimmedWorkspace, bundlePkg) {
  const hasher = createHash('sha256')

  for (const name of ['package.json', 'pnpm-lock.yaml', 'pnpm-workspace.yaml']) {
    const path = join(repoRoot, name)
    if (!existsSync(path)) continue
    hasher.update(name)
    if (name === 'package.json') {
      hasher.update(JSON.stringify(bundlePkg, null, 2))
    }
    else {
      hasher.update(readFileSync(path))
    }
  }

  for (const rel of ['patches', 'vendor', join('native', 'landlock-run'), join('apps', 'cli'), join('apps', 'web')]) {
    const path = join(repoRoot, rel)
    if (existsSync(path)) {
      hashSourceWalk(path, path, hasher, rel.replaceAll('\\', '/'))
    }
  }

  const packagesRoot = join(repoRoot, 'packages')
  for (const group of readdirSync(packagesRoot, { withFileTypes: true })) {
    if (!group.isDirectory() || skipPackageGroups.has(group.name)) continue
    const groupPath = join(packagesRoot, group.name)
    for (const pkg of readdirSync(groupPath, { withFileTypes: true })) {
      if (!pkg.isDirectory()) continue
      const rel = `packages/${group.name}/${pkg.name}`
      hashSourceWalk(join(groupPath, pkg.name), join(groupPath, pkg.name), hasher, rel)
    }
  }

  hasher.update(trimmedWorkspace)
  return hasher.digest('hex')
}

/**
 * @param {string} src
 * @param {string} dest
 */
function copyTree(src, dest) {
  if (!existsSync(src)) return
  if (statSync(src).isFile()) {
    mkdirSync(dirname(dest), { recursive: true })
    copyFileSync(src, dest)
    return
  }
  mkdirSync(dest, { recursive: true })
  cpSync(src, dest, {
    recursive: true,
    dereference: true,
    filter: candidate => shouldCopyEntry(src, candidate),
  })
}

function assertBuiltArtifacts() {
  const cliBin = join(repoRoot, 'apps', 'cli', 'lib', 'bin.js')
  const webIndex = join(repoRoot, 'apps', 'web', 'dist', 'index.html')
  const landlockEntry = join(repoRoot, 'native', 'landlock-run', 'packages', 'entry', 'lib', 'index.js')
  if (!existsSync(cliBin) || !existsSync(webIndex)) {
    throw new Error(
      'Harness build artifacts missing. From repo root run: pnpm run build',
    )
  }
  if (!existsSync(landlockEntry)) {
    throw new Error(
      'landlock-run entry lib missing. From native/landlock-run run: pnpm run build:ts',
    )
  }
}

/** Strip devDependencies so first-run `pnpm install --prod` never resolves demo/test-only workspace refs. */
function stripDevDependencies(root) {
  /** @param {string} dir */
  const walk = dir => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name)
      if (entry.isDirectory()) {
        if (skipDirNames.has(entry.name)) continue
        walk(path)
        continue
      }
      if (entry.name !== 'package.json') continue
      if (!existsSync(path)) continue
      const pkg = JSON.parse(readFileSync(path, 'utf8'))
      if (!pkg.devDependencies) continue
      delete pkg.devDependencies
      writeFileSync(path, `${JSON.stringify(pkg, null, 2)}\n`)
    }
  }
  walk(root)
}

/** @param {string} dir */
function removeTree(dir) {
  if (!existsSync(dir)) return
  try {
    rmSync(dir, { recursive: true, force: true, maxRetries: 3, retryDelay: 200 })
  } catch (error) {
    if (process.platform === 'win32') {
      execSync(`cmd /c rmdir /s /q "${dir.replaceAll('/', '\\')}"`, { stdio: 'ignore' })
      return
    }
    throw error
  }
}

assertBuiltArtifacts()
removeTree(outRoot)
mkdirSync(outRoot, { recursive: true })

for (const name of ['package.json', 'pnpm-lock.yaml', 'pnpm-workspace.yaml']) {
  copyTree(join(repoRoot, name), join(outRoot, name))
}

if (existsSync(join(repoRoot, 'patches'))) {
  copyTree(join(repoRoot, 'patches'), join(outRoot, 'patches'))
}

copyTree(join(repoRoot, 'vendor'), join(outRoot, 'vendor'))
copyTree(join(repoRoot, 'native', 'landlock-run'), join(outRoot, 'native', 'landlock-run'))
copyTree(join(repoRoot, 'apps', 'cli'), join(outRoot, 'apps', 'cli'))
copyTree(join(repoRoot, 'apps', 'web'), join(outRoot, 'apps', 'web'))

const packagesRoot = join(repoRoot, 'packages')
for (const group of readdirSync(packagesRoot, { withFileTypes: true })) {
  if (!group.isDirectory()) continue
  if (skipPackageGroups.has(group.name)) continue
  const groupPath = join(packagesRoot, group.name)
  for (const pkg of readdirSync(groupPath, { withFileTypes: true })) {
    if (!pkg.isDirectory()) continue
    copyTree(join(groupPath, pkg.name), join(outRoot, 'packages', group.name, pkg.name))
  }
}

const trimmedWorkspace = `packages:
  - vendor/*
  - packages/*/*
  - native/landlock-run
  - native/landlock-run/packages/*
  - apps/cli
  - apps/web

linkWorkspacePackages: true

overrides:
  '@deepseek-ai/cosmokit': 'link:vendor/cosmokit'
  '@deepseek-ai/schemastery': 'link:vendor/schemastery'

peerDependencyRules:
  allowedVersions:
    typescript: '>=5 <7'

allowBuilds:
  esbuild: true
  lefthook: true
  node-pty: true
  '@google/genai': false
  protobufjs: false
  node-addon-require-builtin: false
  koffi: true
  '@deepseek-ai/dsh-subprocess-local@file:packages/subprocess/subprocess-local': true

patchedDependencies:
  node-pty@1.1.0: patches/node-pty@1.1.0.patch
`
writeFileSync(join(outRoot, 'pnpm-workspace.yaml'), trimmedWorkspace)

const rootPkg = JSON.parse(readFileSync(join(repoRoot, 'package.json'), 'utf8'))
const bundlePkg = {
  name: '@deepseek-ai/dsh-desktop-bundle',
  private: true,
  version: rootPkg.version,
  packageManager: rootPkg.packageManager ?? 'pnpm@11.7.0',
}
writeFileSync(join(outRoot, 'package.json'), `${JSON.stringify(bundlePkg, null, 2)}\n`)

stripDevDependencies(outRoot)

const manifest = {
  harnessVersion: rootPkg.version,
  bundledAt: new Date().toISOString(),
  contentSha256: hashBundledContent(trimmedWorkspace, bundlePkg),
  method: 'trimmed-monorepo-source',
}
writeFileSync(join(outRoot, '.bundle-manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`)

console.log(`bundle-harness-source: wrote ${outRoot}`)
console.log(`bundle-harness-source: sha256=${manifest.contentSha256}`)
