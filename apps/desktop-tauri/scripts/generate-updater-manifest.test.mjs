import assert from 'node:assert/strict'
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  createManifest,
  normalizedAssets,
  parseArguments,
  resolveNotes,
  writeUpdaterManifest,
} from './generate-updater-manifest.mjs'

const version = '0.1.0-rc.8-0.1'
const repository = 'deepseek-ai/deepseek-harness'
const releaseTag = `desktop-v${version}`
const pubDate = '2026-08-14T00:00:00.000Z'

const expectedAssets = {
  'windows-x86_64': `deepseek-harness-${version}-windows-x64-setup.exe`,
  'windows-i686': `deepseek-harness-${version}-windows-x86-setup.exe`,
  'darwin-x86_64': `deepseek-harness-${version}-macos-x64.app.tar.gz`,
  'darwin-aarch64': `deepseek-harness-${version}-macos-arm64.app.tar.gz`,
  'linux-x86_64': `deepseek-harness-${version}-linux-x64.AppImage`,
}

async function withTempDir(run) {
  const directory = await mkdtemp(join(tmpdir(), 'dsh-updater-'))
  try {
    await run(directory)
  }
  finally {
    await rm(directory, { recursive: true, force: true })
  }
}

async function writeAssets(directory) {
  for (const asset of Object.values(expectedAssets)) {
    await writeFile(join(directory, asset), 'release asset')
    await writeFile(join(directory, `${asset}.sig`), `  signature:${asset}  \n`)
  }
}

test('normalizedAssets maps every required Tauri platform to its release asset', () => {
  assert.deepEqual(normalizedAssets(version), expectedAssets)
})

test('createManifest emits Tauri v2 static updater fields and trimmed signatures', () => {
  const signatures = Object.fromEntries(
    Object.entries(expectedAssets).map(([platform, asset]) => [platform, `  signature:${asset}\n`]),
  )

  assert.deepEqual(createManifest({
    version,
    repository,
    releaseTag,
    notes: 'Release candidate 5',
    pubDate,
    signatures,
  }), {
    version,
    notes: 'Release candidate 5',
    pub_date: pubDate,
    platforms: Object.fromEntries(
      Object.entries(expectedAssets).map(([platform, asset]) => [
        platform,
        {
          signature: `signature:${asset}`,
          url: `https://github.com/${repository}/releases/download/${releaseTag}/${asset}`,
        },
      ]),
    ),
  })
})

test('createManifest rejects invalid versions and publication dates', () => {
  const base = {
    version,
    repository,
    releaseTag,
    notes: '',
    pubDate,
    signatures: Object.fromEntries(Object.keys(expectedAssets).map(platform => [platform, 'sig'])),
  }

  assert.throws(() => createManifest({ ...base, version: 'release-5' }), /invalid version/i)
  assert.throws(() => createManifest({ ...base, pubDate: 'next Friday' }), /invalid pub date/i)
})

test('writeUpdaterManifest rejects a missing normalized asset or signature', async () => {
  await withTempDir(async (directory) => {
    await writeAssets(directory)
    await rm(join(directory, `${expectedAssets['linux-x86_64']}.sig`))

    await assert.rejects(
      writeUpdaterManifest({
        assetsDir: directory,
        outputPath: join(directory, 'latest.json'),
        version,
        repository,
        releaseTag,
        notes: '',
        pubDate,
      }),
      /missing release file.*linux-x64\.AppImage\.sig/i,
    )
  })
})

test('CLI accepts explicit arguments and writes latest.json', async () => {
  await withTempDir(async (directory) => {
    const assetsDir = join(directory, 'assets')
    const outputPath = join(directory, 'nested', 'latest.json')
    await mkdir(assetsDir)
    await writeAssets(assetsDir)

    const result = spawnSync(process.execPath, [
      fileURLToPath(new URL('./generate-updater-manifest.mjs', import.meta.url)),
      '--assets-dir', assetsDir,
      '--output', outputPath,
      '--version', version,
      '--repository', repository,
      '--release-tag', releaseTag,
      '--notes', 'Release candidate 5',
      '--pub-date', pubDate,
    ], { encoding: 'utf8' })

    assert.equal(result.status, 0, result.stderr)
    const manifest = JSON.parse(await readFile(outputPath, 'utf8'))
    assert.equal(manifest.version, version)
    assert.equal(manifest.platforms['darwin-aarch64'].signature,
      `signature:${expectedAssets['darwin-aarch64']}`)
  })
})

test('parseArguments requires every explicit CLI option', () => {
  assert.throws(
    () => parseArguments(['--assets-dir', 'assets']),
    /missing required option: --output/i,
  )
})

test('parseArguments accepts --notes-file instead of --notes', () => {
  const values = parseArguments([
    '--assets-dir', 'assets',
    '--output', 'latest.json',
    '--version', version,
    '--repository', repository,
    '--release-tag', releaseTag,
    '--notes-file', 'release-notes.md',
    '--pub-date', pubDate,
  ])
  assert.equal(values.notesFile, 'release-notes.md')
  assert.equal(values.notes, undefined)
})

test('parseArguments rejects both --notes and --notes-file', () => {
  assert.throws(
    () => parseArguments([
      '--assets-dir', 'assets',
      '--output', 'latest.json',
      '--version', version,
      '--repository', repository,
      '--release-tag', releaseTag,
      '--notes', 'inline',
      '--notes-file', 'release-notes.md',
      '--pub-date', pubDate,
    ]),
    /either --notes or --notes-file/i,
  )
})

test('resolveNotes reads a bilingual notes file', async () => {
  await withTempDir(async (directory) => {
    const notesFile = join(directory, 'release-notes.md')
    await writeFile(notesFile, 'English notes\n\n中文说明\n')
    assert.equal(await resolveNotes({ notesFile }), 'English notes\n\n中文说明\n')
    assert.equal(await resolveNotes({ notes: 'inline' }), 'inline')
  })
})

test('CLI writes latest.json from --notes-file', async () => {
  await withTempDir(async (directory) => {
    const assetsDir = join(directory, 'assets')
    const outputPath = join(directory, 'latest.json')
    const notesFile = join(directory, 'release-notes.md')
    await mkdir(assetsDir)
    await writeAssets(assetsDir)
    await writeFile(notesFile, 'English\n\n中文\n')

    const result = spawnSync(process.execPath, [
      fileURLToPath(new URL('./generate-updater-manifest.mjs', import.meta.url)),
      '--assets-dir', assetsDir,
      '--output', outputPath,
      '--version', version,
      '--repository', repository,
      '--release-tag', releaseTag,
      '--notes-file', notesFile,
      '--pub-date', pubDate,
    ], { encoding: 'utf8' })

    assert.equal(result.status, 0, result.stderr)
    const manifest = JSON.parse(await readFile(outputPath, 'utf8'))
    assert.equal(manifest.notes, 'English\n\n中文\n')
  })
})
