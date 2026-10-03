import { createInterface } from 'node:readline';
import { Runtime } from './runtime.mjs';
// EOF is the existing Rust disconnect receipt; pending mutations reconcile as
// unknown and no subsequent MCP ensureTab can silently create another target.
const runtime = new Runtime({ onDisconnected: () => process.exit(0) });
let active = null;
function reply(id, payload) { const line = JSON.stringify({ id, protocol_version: 1, ...payload }); if (line.length <= 8000000) process.stdout.write(line + '\n'); else process.stdout.write(JSON.stringify({ id, protocol_version: 1, ok: false, outcome_unknown: false, cancelled: false, error: 'browser reply exceeds limit' }) + '\n'); }
createInterface({ input: process.stdin, crlfDelay: Infinity }).on('line', async line => {
  let request;
  try {
    if (line.length > 65536) throw new Error('request too large');
    request = JSON.parse(line);
    if (request.method === 'cancel') {
      // No replay: terminate the connection, never close an extension tab. The
      // Rust host classifies every in-flight mutation as unknown on worker EOF.
      process.exit(0);
    }
    if (active !== null) throw new Error('NOT_EXECUTED: browser busy');
    active = request.id;
    const result = await runtime.execute(request);
    reply(request.id, { ok: true, result, outcome_unknown: false, cancelled: false });
    if (request.method === 'detach') process.exit(0);
  } catch (error) {
    const message = String(error.message || error).slice(0, 2000);
    // Owner issue19: lifecycle may have opened/closed a window before failure.
    // A false unknown costs reconciliation; false certainty can replay effects.
    const mutation = ['open', 'detach', 'navigate', 'click', 'type', 'key', 'scroll'].includes(request?.method);
    reply(request?.id || 0, { ok: false, error: message, outcome_unknown: mutation && error.beforeDispatch !== true, cancelled: false });
  } finally { if (active === request?.id) active = null; }
});
process.stdin.on('end', () => process.exit(0));
