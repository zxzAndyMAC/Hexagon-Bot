import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { chmodSync, existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const computerUsePackage = fileURLToPath(new URL('../native/computer-use', import.meta.url))
const sha256 = value => createHash('sha256').update(value).digest('hex')

// QA 2026-10-06 / action1235: keep the upstream revision fixed; never silently
// carry this concurrency patch onto a different owner implementation.
export function applyCapturePatch(checkout, manifest, patchFile) {
  const head = spawnSync('git', ['-C', checkout, 'rev-parse', 'HEAD'], { encoding: 'utf8' })
  if (head.status !== 0 || head.stdout.trim() !== manifest.revision) {
    throw new Error('Computer-use SDK revision differs from the reviewed capture patch')
  }
  const source = path.join(checkout, manifest.file)
  const current = sha256(readFileSync(source))
  if (current === manifest.after) return
  if (current !== manifest.before) throw new Error('Computer-use capture source has unreviewed changes')
  const check = spawnSync('git', ['-C', checkout, 'apply', '--unidiff-zero', '--check', patchFile], { encoding: 'utf8' })
  if (check.status !== 0) throw new Error('Computer-use capture patch does not apply cleanly')
  // SwiftPM checkouts are read-only. Only the hash-verified patch target changes.
  chmodSync(source, 0o644)
  const applied = spawnSync('git', ['-C', checkout, 'apply', '--unidiff-zero', patchFile], { encoding: 'utf8' })
  if (applied.status !== 0 || sha256(readFileSync(source)) !== manifest.after) {
    throw new Error('Computer-use capture patch verification failed')
  }
}

export function prepareComputerUse() {
  const patchDirectory = path.join(computerUsePackage, 'patches')
  const manifest = JSON.parse(readFileSync(path.join(patchDirectory, 'manifest.json'), 'utf8'))
  const resolved = JSON.parse(readFileSync(path.join(computerUsePackage, 'Package.resolved'), 'utf8'))
  if (!resolved.pins.some(pin => pin.identity === 'peekaboo' && pin.state.revision === manifest.revision)) {
    throw new Error('Computer-use resolved SDK does not match the reviewed capture patch')
  }
  const checkout = path.join(computerUsePackage, '.build/checkouts/Peekaboo')
  if (!existsSync(checkout)) {
    const result = spawnSync('swift', ['package', '--package-path', computerUsePackage, 'resolve'], { stdio: 'inherit' })
    if (result.status !== 0) throw new Error('Computer-use dependency resolution failed')
  }
  applyCapturePatch(checkout, manifest, path.join(patchDirectory, manifest.patch))
}
