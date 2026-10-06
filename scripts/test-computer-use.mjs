import { spawnSync } from 'node:child_process'
import { computerUsePackage, prepareComputerUse } from './prepare-computer-use.mjs'

if (process.platform === 'darwin') {
  prepareComputerUse()
  const result = spawnSync('swift', ['test', '--package-path', computerUsePackage,
    '--jobs', '4', '--disable-automatic-resolution'], { stdio: 'inherit' })
  process.exit(result.status ?? 1)
}
console.log('Native computer-use tests skipped: macOS is required')
