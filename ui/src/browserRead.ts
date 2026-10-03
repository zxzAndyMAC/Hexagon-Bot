/** Owner incident 2026-10-02: passive picker/preview reads share the worker
 * with role actions. Only these explicit scheduling misses may be deferred;
 * target changes, revoked access and unknown failures must still clear pixels.
 * Match the host transport messages, never the translated generic error text. */
export function isBrowserReadBusy(error: unknown): boolean {
  const message = error && typeof error === 'object' && 'message' in error ? error.message : error
  return message === 'browser busy; local preview yields to role work'
    || message === 'NOT_EXECUTED: browser busy'
    || message === 'NOT_EXECUTED: preview throttled'
}
