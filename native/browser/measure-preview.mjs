import { Runtime } from './runtime.mjs';
import { randomUUID } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
const runtime = new Runtime({ headless: true });
const session_id = randomUUID(); let id = 0, frames = 0;
const run = (method, params = {}) => runtime.execute({ id: ++id, method, session_id, deadline: Date.now() + 60000, params });
const pause = ms => new Promise(r => setTimeout(r, ms));
const measure = async (name, body) => { const start = performance.now(), cpu = process.cpuUsage(), count = frames; await body(); const used = process.cpuUsage(cpu); return { phase: name, wall_ms: performance.now() - start, node_cpu_ms: (used.user + used.system) / 1000, node_rss_bytes: process.memoryUsage().rss, frames: frames - count }; };
try {
  await run('open', { mode: 'managed' });
  const output = [];
  output.push(await measure('idle', () => pause(1500)));
  let maxFrameBytes = 0;
  output.push(await measure('visible_2fps', async () => { for (let i = 0; i < 10; i++) { const frame = await run('preview'); frames++; maxFrameBytes = Math.max(maxFrameBytes, frame.data_url.length); await pause(500); } }));
  output.push(await measure('hidden_no_capture', () => pause(1500)));
  const report = { measured_at: new Date().toISOString(), scope: 'Real isolated headless Chromium and MCP wrapper; CPU/RSS are Node host only, not browser process or native app E2E. Full viewport 1280x800 JPEG quality35; UI scales, <=1MiB encoded source; no capture subscription.', max_frame_data_url_bytes: maxFrameBytes, phases: output };
  if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} finally { try { await run('detach'); } catch {} await runtime.client?.close(); }
