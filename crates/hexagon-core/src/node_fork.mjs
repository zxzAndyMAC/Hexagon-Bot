// 2026-09-29 official Node/libuv aborts when Seatbelt rejects posix_spawn.
// An explicit uid selects libuv's supported fork/exec path *before* spawn setup.
// Keep identity unchanged. This is compatibility, not confinement: the kernel
// still denies setsid/setpgid/posix_spawn even if a program bypasses this shim.
import cp from 'node:child_process';
import {syncBuiltinESMExports} from 'node:module';

if (process.getuid() === process.geteuid()) {
  const uid = process.getuid();
  const preload = `--import=${import.meta.url}`;
  const nodeOptions = value => {
    const text = String(value ?? '');
    return text.includes(preload) ? text : `${preload} ${text}`;
  };
  const withUid = options => {
    const copy = {...options};
    copy.uid ??= uid;
    // A caller-supplied env used to lose the preload: the child then aborted
    // when starting a grandchild. Carry only this host shim into that env;
    // never merge discarded host variables back into an explicit environment.
    const env = copy.env ?? process.env;
    if (typeof env === 'object' && !Array.isArray(env)) {
      copy.env = {...env, NODE_OPTIONS: nodeOptions(env.NODE_OPTIONS)};
    }
    // Async spawn reaches this hook after Node has normalized env to envPairs.
    if (Array.isArray(copy.envPairs)) {
      const value = copy.envPairs.find(pair => pair.startsWith('NODE_OPTIONS='));
      copy.envPairs = copy.envPairs.filter(pair => !pair.startsWith('NODE_OPTIONS='));
      copy.envPairs.push(`NODE_OPTIONS=${nodeOptions(value?.slice(13))}`);
    }
    return copy;
  };
  const spawn = cp.ChildProcess.prototype.spawn;
  cp.ChildProcess.prototype.spawn = function(options) {
    return spawn.call(this, withUid(options));
  };

  // Sync APIs have lexical calls to spawnSync; patch each public entry point.
  // Leave invalid arguments to Node's validators and don't mutate caller options.
  for (const name of ['spawnSync', 'execFileSync', 'execSync']) {
    const original = cp[name];
    cp[name] = function(...args) {
      const index = name === 'execSync' ? 1
        : args[1] != null && typeof args[1] === 'object' && !Array.isArray(args[1]) ? 1 : 2;
      const options = args[index];
      if (options === undefined || (options === null && name !== 'spawnSync') || (options !== null && typeof options === 'object' && !Array.isArray(options))) {
        args[index] = withUid(options ?? {});
      }
      return Reflect.apply(original, this, args);
    };
  }
  syncBuiltinESMExports();
}
