import { createHash } from 'node:crypto'
import { readFile, writeFile, mkdir, copyFile, chmod } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { homedir } from 'node:os'
import path from 'node:path'

// Official Node release SHASUMS256.txt verified 2026-10-02. Pin both version
// and archive digest; copying a Homebrew node alone leaves external dylib deps.
const version = '22.23.1'
const digests = {
  'darwin-arm64': 'ef28d8fab2c0e4314522d4bb1b7173270aa3937e93b92cb7de79c112ac1fa953',
  'darwin-x64': 'b8da981b8a0b1241b70249204916da76c63573ddf5814dbd2d1e41069105cb81',
  'linux-arm64': '543fa39e57d4c07855939459a323f4deb9a79dd1bb45e6e99458b0f2de10db8d',
  'linux-x64': '7a8cb04b4a1df4eaf432125324b81b29a088e73570a23259a8de1c65d07fc129',
}
export async function prepareNode(destination) {
  const platform = `${process.platform}-${process.arch}`
  const expected = digests[platform]
  if (!expected) throw new Error(`No verified standalone browser Node runtime for ${platform}`)
  const name = `node-v${version}-${platform}`
  const cache = path.join(homedir(), '.cache', 'hexagon-browser-runtime')
  await mkdir(cache, { recursive: true, mode: 0o700 })
  const archive = path.join(cache, `${name}.tar.gz`)
  let bytes
  try { bytes = await readFile(archive) } catch {}
  if (!bytes || createHash('sha256').update(bytes).digest('hex') !== expected) {
    const response = await fetch(`https://nodejs.org/download/release/v${version}/${name}.tar.gz`)
    if (!response.ok) throw new Error(`Node runtime download failed: ${response.status}`)
    bytes = Buffer.from(await response.arrayBuffer())
    if (createHash('sha256').update(bytes).digest('hex') !== expected) throw new Error('Node runtime checksum mismatch')
    await writeFile(archive, bytes, { mode: 0o600 })
  }
  const extract = spawnSync('tar', ['-xzf', archive, '-C', cache], { stdio: 'inherit' })
  if (extract.status !== 0) throw new Error('Could not extract verified Node runtime')
  const node = path.join(cache, name, 'bin', 'node')
  if (process.platform === 'darwin') {
    const linked = spawnSync('/usr/bin/otool', ['-L', node], { encoding: 'utf8' })
    if (linked.status !== 0 || linked.stdout.split('\n').slice(1).filter(Boolean).some(line => !/^\s+\/(?:usr\/lib|System\/Library)\//.test(line))) throw new Error('Browser Node runtime has non-system dynamic dependencies')
  }
  await mkdir(path.dirname(destination), { recursive: true })
  await copyFile(node, destination); await chmod(destination, 0o755)
  return destination
}
