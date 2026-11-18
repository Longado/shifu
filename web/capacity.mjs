import os from 'node:os';
import { execFileSync } from 'node:child_process';

// 2026-11-18 measured about 180 MiB per task on a documentation page; rounded up for heavier pages.
export const TASK_MEMORY = 256 * 2 ** 20;
// Browser tasks may use at most this share of currently available memory; agents and apps need the rest.
export const MEMORY_SHARE = 0.25;

function availableMemory() {
  if (process.platform === 'darwin') {
    // os.freemem() on macOS counts only free pages and ignores reclaimable cache; this is the
    // same "free percentage" that memory_pressure reports.
    const level = Number(execFileSync('sysctl', ['-n', 'kern.memorystatus_level'], { encoding: 'utf8' }).trim());
    if (Number.isFinite(level) && level > 0 && level <= 100) return os.totalmem() * level / 100;
  }
  return os.freemem();
}

export function machineCapacity({ available, cpus }) {
  const byMemory = Math.floor(available * MEMORY_SHARE / TASK_MEMORY);
  return { limit: Math.max(1, Math.min(cpus, byMemory)),
    basis: { availableMiB: Math.round(available / 2 ** 20), byMemory, cpus, taskMiB: TASK_MEMORY / 2 ** 20, share: MEMORY_SHARE } };
}

export function readCapacity(env = process.env) {
  if (env.SHIFU_WEB_MAX_TASKS !== undefined) {
    const limit = Number(env.SHIFU_WEB_MAX_TASKS);
    if (!Number.isInteger(limit) || limit < 1 || limit > 32) throw new Error('SHIFU_WEB_MAX_TASKS须为1到32的整数。');
    return { limit, basis: { override: true } };
  }
  return machineCapacity({ available: availableMemory(), cpus: os.cpus().length });
}
