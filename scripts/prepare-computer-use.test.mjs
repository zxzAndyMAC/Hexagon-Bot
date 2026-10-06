import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { applyCapturePatch } from './prepare-computer-use.mjs'

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'hexagon-capture-patch-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const checkout = path.join(root, 'sdk')
  mkdirSync(checkout)
  const git = args => execFileSync('git', ['-C', checkout, ...args], { encoding: 'utf8' }).trim()
  git(['init', '--quiet'])
  writeFileSync(path.join(checkout, 'gate.swift'), 'before\n')
  git(['add', 'gate.swift'])
  git(['-c', 'user.name=Fixture', '-c', 'user.email=fixture@localhost', 'commit', '--quiet', '-m', 'fixture'])
  const patch = path.join(root, 'capture.patch')
  writeFileSync(patch, '--- a/gate.swift\n+++ b/gate.swift\n@@ -1 +1 @@\n-before\n+after\n')
  const hash = value => createHash('sha256').update(value).digest('hex')
  return { checkout, patch, source: path.join(checkout, 'gate.swift'),
    manifest: { revision: git(['rev-parse', 'HEAD']), file: 'gate.swift',
      before: hash('before\n'), after: hash('after\n') } }
}

test('clean fixed dependency is patched and repeated preparation is harmless', t => {
  const f = fixture(t)
  applyCapturePatch(f.checkout, f.manifest, f.patch)
  assert.equal(readFileSync(f.source, 'utf8'), 'after\n')
  applyCapturePatch(f.checkout, f.manifest, f.patch)
  assert.equal(readFileSync(f.source, 'utf8'), 'after\n')
})

test('revision drift and local source changes refuse preparation without overwriting', t => {
  const f = fixture(t)
  assert.throws(() => applyCapturePatch(f.checkout, { ...f.manifest, revision: 'unknown' }, f.patch), /revision differs/)
  assert.equal(readFileSync(f.source, 'utf8'), 'before\n')
  writeFileSync(f.source, 'local change\n')
  assert.throws(() => applyCapturePatch(f.checkout, f.manifest, f.patch), /unreviewed changes/)
  assert.equal(readFileSync(f.source, 'utf8'), 'local change\n')
})

test('malformed patch and unexpected result both stop the build', t => {
  const f = fixture(t)
  writeFileSync(f.patch, 'not a patch\n')
  assert.throws(() => applyCapturePatch(f.checkout, f.manifest, f.patch), /does not apply/)
  assert.equal(readFileSync(f.source, 'utf8'), 'before\n')
  writeFileSync(f.patch, '--- a/gate.swift\n+++ b/gate.swift\n@@ -1 +1 @@\n-before\n+wrong\n')
  assert.throws(() => applyCapturePatch(f.checkout, f.manifest, f.patch), /verification failed/)
})
