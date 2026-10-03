import { spawnSync } from 'node:child_process'
import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { prepareNode } from './browser-node-runtime.mjs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

// Issue 12: use the lockfile, never latest npx, and ship the same Node/browser
// runtime used by the app. A failed install aborts the build, not permissions.
const root = fileURLToPath(new URL('..', import.meta.url))
const source = path.join(root, 'native/browser')
const release = process.argv.includes('--release')
function run(executable, args, cwd = source, env = process.env) {
  const result = spawnSync(executable, args, { cwd, env, stdio: 'inherit' })
  if (result.status !== 0) process.exit(result.status ?? 1)
}
const lockHash = createHash('sha256').update(readFileSync(path.join(source, 'package-lock.json'))).digest('hex')
const marker = path.join(source, 'node_modules/.hexagon-lock-sha256')
if (!existsSync(marker) || readFileSync(marker, 'utf8') !== lockHash) {
  run(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['ci', '--ignore-scripts'])
  writeFileSync(marker, lockHash)
}
run(process.execPath, [path.join(source, 'build-react-source.mjs')])
if (!release) {
  run(process.execPath, [path.join(source, 'node_modules/playwright/cli.js'), 'install', 'chromium'], source, { ...process.env, PLAYWRIGHT_SKIP_BROWSER_GC: '1' })
} else {
  const destination = path.join(root, 'src-tauri/resources/browser')
  mkdirSync(destination, { recursive: true })
  for (const name of ['worker.mjs', 'runtime.mjs', 'page.mjs', 'selection.mjs', 'react-source.generated.mjs', 'package.json', 'package-lock.json']) copyFileSync(path.join(source, name), path.join(destination, name))
  cpSync(path.join(source, 'node_modules'), path.join(destination, 'node_modules'), { recursive: true })
  const node = path.join(destination, process.platform === 'win32' ? 'node.exe' : 'node')
  await prepareNode(node)
  run(node, [path.join(destination, 'node_modules/playwright/cli.js'), 'install', 'chromium'], destination, { ...process.env, PLAYWRIGHT_BROWSERS_PATH: path.join(destination, 'browsers'), PLAYWRIGHT_SKIP_BROWSER_GC: '1' })
}
