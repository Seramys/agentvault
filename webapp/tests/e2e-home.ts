import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * The E2E fixture starts `agentvault web` against a temp `AGENTVAULT_HOME` and
 * exposes it here; the bootstrap token is read from that home, never hardcoded.
 */
export function e2eHome(): string {
  const home = process.env.AGENTVAULT_E2E_HOME;
  if (!home) {
    throw new Error('set AGENTVAULT_E2E_HOME to the temp home used by `agentvault web`');
  }
  return home;
}

export function webToken(): string {
  const token = readFileSync(join(e2eHome(), 'web.token'), 'utf8').trim();
  if (!token) throw new Error('web.token is empty');
  return token;
}