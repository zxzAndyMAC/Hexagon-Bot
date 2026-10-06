import sourceBundle from './react-source.generated.mjs';
import { randomUUID } from 'node:crypto';
import { createConnection } from '@playwright/mcp';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { pageOperation } from './page.mjs';

const ACTIONS = new Set(['navigate', 'click', 'type', 'key', 'scroll']);
const METHODS = new Set(['open', 'observe', 'diagnostics', 'metadata', 'preview', 'focus', 'detach', ...ACTIONS, 'selection.start', 'selection.poll', 'selection.stop']);
const KEY = /^(Enter|Tab|Escape|Backspace|Delete|ArrowUp|ArrowDown|ArrowLeft|ArrowRight|Home|End|PageUp|PageDown|Space|ControlOrMeta\+[acvxyz])$/;
export function validate(request) {
  try {
  if (!request || !Number.isSafeInteger(request.id) || request.id < 1 || !METHODS.has(request.method)) throw new Error('NOT_EXECUTED: invalid browser request');
  if (typeof request.session_id !== 'string' || !/^[a-f0-9-]{36}$/.test(request.session_id)) throw new Error('NOT_EXECUTED: invalid session');
  if (!Number.isSafeInteger(request.deadline) || request.deadline < Date.now()) throw new Error('NOT_EXECUTED: expired request');
  const p = request.params || {};
  if (request.method === 'open' && !['managed', 'extension'].includes(p.mode)) throw new Error('NOT_EXECUTED: invalid browser mode');
  if (request.method === 'type' && (typeof p.text !== 'string' || p.text.length > 16000)) throw new Error('NOT_EXECUTED: invalid text');
  if (request.method === 'navigate') { const url = new URL(p.url); if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password) throw new Error('NOT_EXECUTED: invalid URL'); }
  if (request.method === 'key' && !KEY.test(p.key)) throw new Error('NOT_EXECUTED: invalid key');
  if (request.method === 'scroll' && (!['up', 'down'].includes(p.direction) || !Number.isInteger(p.amount) || p.amount < 1 || p.amount > 5)) throw new Error('NOT_EXECUTED: invalid scroll');
  if (ACTIONS.has(request.method) && (typeof p.snapshot_id !== 'string' || !p.snapshot_id.startsWith('bs1-'))) throw new Error('NOT_EXECUTED: snapshot required');
  if (['click', 'type', 'key'].includes(request.method) && !/^be\d{1,3}$/.test(p.element_id)) throw new Error('NOT_EXECUTED: observed element required');
  return p;
  } catch(error) { error.beforeDispatch = true; throw error; }
}

export class Runtime {
  constructor({ headless = false, onDisconnected = () => {} } = {}) { this.headless = headless; this.client = null; this.session = null; this.busy = false; this.lastPreview = 0; this.closed = false; this.detaching = false; this.boundPage = null; this.onDisconnected = onDisconnected; }
  async fixed(fn, ...args) {
    if (this.closed || this.boundPage?.isClosed()) { const error = new Error('NOT_EXECUTED: bound browser tab closed; open a new session'); error.beforeDispatch = true; throw error; }
    const encoded = Buffer.from(JSON.stringify(args)).toString('base64');
    const result = await this.client.callTool({ name: 'browser_run_code_unsafe', arguments: { code: `(page) => (${fn.toString()})(page, ...JSON.parse(Buffer.from('${encoded}', 'base64').toString()))` } });
    const text = result.content.filter(c => c.type === 'text').map(c => c.text).join('\n');
    if (result.isError) throw new Error(text.slice(0, 2000));
    const match = text.match(/### Result\n([^]*?)(?:\n### |$)/);
    if (!match) throw new Error('Invalid browser runtime response');
    const value = JSON.parse(match[1]);
    if (value.__host_error) { const error = new Error(value.__host_error.message); error.beforeDispatch = !value.__host_error.dispatched; throw error; }
    return value;
  }
  async execute(request) {
    const p = validate(request);
    if (this.busy) throw new Error('NOT_EXECUTED: browser busy');
    if (request.method !== 'open' && this.session?.session_id !== request.session_id) throw new Error('NOT_EXECUTED: stale browser session');
    if (request.method === 'preview' && Date.now() - this.lastPreview < 500) throw new Error('NOT_EXECUTED: preview throttled');
    this.busy = true;
    try {
      if (request.method === 'open') {
        if (this.session) throw new Error('NOT_EXECUTED: detach current browser first');
        // Owner incident 2026-10-06: fixed viewport emulation defaults to DPR=1,
        // so even scale:'device' still captures a half-size Retina surface (issue17).
        // Visible windows use native display pixels; only headless fixtures emulate size.
        const server = await createConnection({ extension: p.mode === 'extension', browser: { browserName: 'chromium', isolated: p.mode === 'managed', launchOptions: { headless: this.headless }, contextOptions: { viewport: this.headless ? { width: 1280, height: 800 } : null, acceptDownloads: false } }, webmcp: false, saveSession: false, codegen: 'none', snapshot: { mode: 'none' }, timeouts: { action: 5000, navigation: 15000, settle: 0 } });
        const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
        this.client = new Client({ name: 'hexagon-owner-browser', version: '1.0.0' });
        await server.connect(serverTransport);
        await this.client.connect(clientTransport);
        this.server = server;
        this.session = { session_id: request.session_id, mode: p.mode };
        this.labels = p.labels;
        // Issue12 / 2026-10-02: MCP ensureTab creates a replacement for closed
        // tabs. Capture the public Page once through the pinned unsafe VM's
        // shared Buffer reference; no token/callback enters page JavaScript.
        // All subsequent MCP calls check the original Page before ensureTab.
        const bridge = 'hexagon-page-' + randomUUID();
        Buffer[bridge] = page => {
          this.boundPage = page;
          page.once('close', () => {
            this.closed = true;
            if (this.session) this.session.connected = false;
            if (!this.detaching) this.onDisconnected();
          });
        };
        try {
          await this.fixed(async (page, bridge) => {
            if (typeof Buffer[bridge] !== 'function') throw new Error('Browser liveness bridge unavailable');
            Buffer[bridge](page);
            return { bound: true };
          }, bridge);
        } finally { delete Buffer[bridge]; }

      }
      const op = request.method === 'open' ? 'metadata' : request.method;
      if (op === 'detach') this.detaching = true;
      const input = { ...p, op, mode: this.session.mode, deadline: request.deadline, new_tab_id: randomUUID(), new_snapshot_id: 'bs1-' + randomUUID() };
      let result;
      if (op.startsWith('selection.')) {
        const module = await import('./selection.mjs');
        const metadata = await this.fixed(pageOperation, { ...input, op: 'metadata' });
        const fn = op === 'selection.start' ? module.installSelection : op === 'selection.poll' ? module.pollSelection : module.stopSelection;
        if (op === 'selection.start' && p.labels) this.labels = p.labels;
        result = await this.fixed(fn, { session_id: request.session_id, ...metadata, active: p.active }, p.labels, sourceBundle);
        const after = await this.fixed(pageOperation, { ...input, op: 'metadata' });
        if (after.tab_id !== metadata.tab_id || after.navigation_generation !== metadata.navigation_generation) throw new Error('NOT_EXECUTED: selection navigation changed');
        result = { ...result, session_id: request.session_id, ...after };
      } else result = await this.fixed(pageOperation, input);
      if (op === 'preview') this.lastPreview = Date.now();
      if (op === 'detach') { await this.client.close(); this.session = null; }
      else this.session = { ...this.session, tab_id: result.tab_id, navigation_generation: result.navigation_generation, url: result.url, title: result.title };
      // Issue19: agent-opened sessions get localized picker labels only when
      // the owner invokes selection; do not inject an unlabeled toolbar.
      if (this.labels && ['open', 'navigate', 'metadata'].includes(request.method)) {
        const { installSelection } = await import('./selection.mjs');
        await this.fixed(installSelection, { session_id: request.session_id, ...result, active: false }, this.labels, sourceBundle).catch(() => {}); // Decorative picker failure never reverses a dispatched navigation receipt.
      }
      return { ...result, session_id: request.session_id, mode: p.mode || this.session?.mode, connected: op !== 'detach' };
    } finally { this.busy = false; }
  }
}
