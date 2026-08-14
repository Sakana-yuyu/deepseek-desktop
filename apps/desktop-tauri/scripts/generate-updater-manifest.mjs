import { access, mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const PLATFORM_ASSET_SUFFIXES = {
  'windows-x86_64': 'windows-x64-setup.exe',
  'windows-i686': 'windows-x86-setup.exe',
  'darwin-x86_64': 'macos-x64.app.tar.gz',
  'darwin-aarch64': 'macos-arm64.app.tar.gz',
  'linux-x86_64': 'linux-x64.AppImage',
}

const OPTIONS = {
  '--assets-dir': 'assetsDir',
  '--output': 'outputPath',
  '--version': 'version',
  '--repository': 'repository',
  '--release-tag': 'releaseTag',
  '--notes': 'notes',
  '--pub-date': 'pubDate',
}

/** @param {string} version @returns {Record<string, string>} */
export function normalizedAssets(version) {
  return Object.fromEntries(
    Object.entries(PLATFORM_ASSET_SUFFIXES).map(([platform, suffix]) => [
      platform,
      `deepseek-harness-${version}-${suffix}`,
    ]),
  )
}

/** @param {string} version */
export function validateVersion(version) {
  const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/
  if (!semver.test(version)) throw new Error(`Invalid version: ${version}`)
}

/** @param {string} pubDate */
export function validatePubDate(pubDate) {
  if (!/^\d{4}-\d{2}-\d{2}T/.test(pubDate) || !Number.isFinite(Date.parse(pubDate))) {
    throw new Error(`Invalid pub date: ${pubDate}`)
  }
}

/**
 * @param {{ version: string, repository: string, releaseTag: string, notes: string, pubDate: string, signatures: Record<string, string> }} options
 * @returns {{ version: string, notes: string, pub_date: string, platforms: Record<string, { signature: string, url: string }> }}
 */
export function createManifest(options) {
  const { version, repository, releaseTag, notes, pubDate, signatures } = options
  validateVersion(version)
  validatePubDate(pubDate)

  const platforms = Object.fromEntries(
    Object.entries(normalizedAssets(version)).map(([platform, asset]) => {
      const signature = signatures[platform]?.trim()
      if (!signature) throw new Error(`Missing signature for platform: ${platform}`)
      return [platform, {
        signature,
        url: `https://github.com/${repository}/releases/download/${releaseTag}/${asset}`,
      }]
    }),
  )

  return { version, notes, pub_date: pubDate, platforms }
}

/**
 * @param {string[]} args
 * @returns {{ assetsDir: string, outputPath: string, version: string, repository: string, releaseTag: string, notes: string, pubDate: string }}
 */
export function parseArguments(args) {
  const values = {}
  for (let index = 0; index < args.length; index += 2) {
    const option = args[index]
    const key = OPTIONS[option]
    if (!key) throw new Error(`Unknown option: ${option}`)
    const value = args[index + 1]
    if (value === undefined) throw new Error(`Missing value for option: ${option}`)
    values[key] = value
  }
  for (const [option, key] of Object.entries(OPTIONS)) {
    if (values[key] === undefined) throw new Error(`Missing required option: ${option}`)
  }
  return values
}

/**
 * @param {{ assetsDir: string, outputPath: string, version: string, repository: string, releaseTag: string, notes: string, pubDate: string }} options
 * @returns {Promise<void>}
 */
export async function writeUpdaterManifest(options) {
  const assets = normalizedAssets(options.version)
  const signatures = {}
  for (const [platform, asset] of Object.entries(assets)) {
    const assetPath = resolve(options.assetsDir, asset)
    const signaturePath = `${assetPath}.sig`
    for (const path of [assetPath, signaturePath]) {
      try {
        await access(path)
      }
      catch {
        throw new Error(`Missing release file: ${path}`)
      }
    }
    signatures[platform] = await readFile(signaturePath, 'utf8')
  }

  const manifest = createManifest({ ...options, signatures })
  await mkdir(dirname(resolve(options.outputPath)), { recursive: true })
  await writeFile(options.outputPath, `${JSON.stringify(manifest, null, 2)}\n`)
}

const isMain = process.argv[1]
  && resolve(process.argv[1]) === fileURLToPath(import.meta.url)

if (isMain) {
  try {
    const options = parseArguments(process.argv.slice(2))
    await writeUpdaterManifest(options)
    console.log(`generate-updater-manifest: wrote ${resolve(options.outputPath)}`)
  }
  catch (error) {
    console.error(error instanceof Error ? error.message : error)
    process.exitCode = 1
  }
}
