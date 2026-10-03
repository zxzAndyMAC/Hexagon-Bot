import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

// Desktop ticket 07: lock public AutomationKit, never install a rolling CLI or
// borrow another application's Bridge. Non-macOS retains explicit unsupported UI.
if (process.platform === 'darwin') {
  const root = fileURLToPath(new URL('..', import.meta.url))
  const configuration = process.argv.includes('--release') ? 'release' : 'debug'
  const result = spawnSync('swift', ['build', '--package-path', path.join(root, 'native/computer-use'), '--configuration', configuration, '--jobs', '4', '--disable-automatic-resolution'], { stdio: 'inherit' })
  if (result.status !== 0) process.exit(result.status ?? 1)
  const target = path.join(root, 'target', configuration)
  mkdirSync(target, { recursive: true })
  copyFileSync(path.join(root, 'native/computer-use/.build', configuration, 'libHexagonComputerUse.dylib'), path.join(target, 'libHexagonComputerUse.dylib'))
}
