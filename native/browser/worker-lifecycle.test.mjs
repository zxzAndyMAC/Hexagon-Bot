import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { createInterface } from 'node:readline';

// Owner issue19 / 2026-10-02: a failed managed launch may already have opened a
// window. The role must reconcile, never infer a safe automatic lifecycle retry.
test('worker marks an attempted session launch unknown but validation refusal not executed', { timeout: 15000 }, async t => {
  const worker = spawn(process.execPath, [new URL('./worker.mjs', import.meta.url).pathname], {
    env: { ...process.env, PLAYWRIGHT_BROWSERS_PATH: '/nonexistent-hexagon-lifecycle-fixture' },
    stdio: ['pipe', 'pipe', 'ignore'],
  });
  t.after(() => worker.kill());
  const replies = createInterface({ input: worker.stdout })[Symbol.asyncIterator]();
  const run = async mode => {
    worker.stdin.write(JSON.stringify({ id: mode === 'invalid' ? 1 : 2, method: 'open', session_id: randomUUID(), deadline: Date.now() + 10000, params: { mode } }) + '\n');
    const reply = await replies.next();
    assert.equal(reply.done, false);
    return JSON.parse(reply.value);
  };
  const refused = await run('invalid');
  assert.equal(refused.ok, false);
  assert.equal(refused.outcome_unknown, false);
  const attempted = await run('managed');
  assert.equal(attempted.ok, false);
  assert.equal(attempted.outcome_unknown, true);
});
