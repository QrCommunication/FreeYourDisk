#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { stat } from 'node:fs/promises';
import { resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const DEADLINE_MS = 120 * 60 * 1000;
const UPLOAD_TIMEOUT_MS = 300_000;
const PROCESS_GRACE_MS = 30_000;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function requireName(value, name) {
  if (typeof value !== 'string' || !value.trim() || value.includes('\0')) {
    throw new Error(`${name} must be a nonempty string.`);
  }
}

function requireInteger(value, name, minimum, maximum) {
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    throw new Error(`${name} must be an integer between ${minimum} and ${maximum}.`);
  }
}

function parseResponse(stdout) {
  let response;
  try {
    response = JSON.parse(String(stdout ?? ''));
  } catch {
    throw new Error('Notarization returned invalid JSON.');
  }
  if (!response || typeof response !== 'object' || Array.isArray(response)
      || typeof response.id !== 'string' || !UUID.test(response.id)) {
    throw new Error('Notarization returned an invalid submission identifier.');
  }
  return response;
}

function isTransientFailure(result) {
  const stderr = String(result.stderr ?? '');
  if (/\b(?:401|403)\b|invalid credentials|authentication failed/i.test(stderr)) return false;
  return result.status === 124 || result.error?.code === 'ETIMEDOUT'
    || /NSURLError(?:Domain)?[^\n]*?(?:Code\s*[=:]\s*)?-(?:1005|1001)\b/i.test(stderr);
}

function processSucceeded(result) {
  return result?.status === 0 && !result.error;
}

export async function notarizeDmg(
  { dmgPath, profile, keychainPath },
  {
    run = spawnSync,
    sleep = delay,
    now = Date.now,
    onSubmission = () => {},
    attempts = 12,
    waitTimeoutSeconds = 600,
    retryDelaySeconds = 15,
  } = {},
) {
  requireName(dmgPath, 'dmgPath');
  requireName(profile, 'profile');
  requireName(keychainPath, 'keychainPath');
  requireInteger(attempts, 'attempts', 1, 12);
  requireInteger(waitTimeoutSeconds, 'waitTimeoutSeconds', 1, 600);
  requireInteger(retryDelaySeconds, 'retryDelaySeconds', 0, 60);
  if ([run, sleep, now, onSubmission].some((dependency) => typeof dependency !== 'function')) {
    throw new Error('Process, sleep, clock and submission observer dependencies must be functions.');
  }

  const executable = process.env.XCRUN_BIN || 'xcrun';
  const deadline = now() + DEADLINE_MS;
  const remainingTime = () => {
    const remaining = deadline - now();
    if (!Number.isFinite(remaining) || remaining <= 0) {
      throw new Error('Notarization exceeded its 120-minute deadline.');
    }
    return remaining;
  };
  const credentials = ['--keychain-profile', profile, '--keychain', keychainPath,
    '--output-format', 'json'];
  const execute = async (args, timeout) => {
    const result = await run(executable, ['notarytool', ...args, ...credentials], {
      encoding: 'utf8',
      timeout: Math.min(timeout, remainingTime()),
      killSignal: 'SIGKILL',
      maxBuffer: 1024 * 1024,
    });
    remainingTime();
    if (!result || typeof result !== 'object') {
      throw new Error('Notarization process did not return a result.');
    }
    return result;
  };

  // Never retry an upload: an interrupted submission may already exist at Apple.
  const submission = await execute(['submit', dmgPath], UPLOAD_TIMEOUT_MS);
  if (!processSucceeded(submission)) throw new Error('Notarization submission failed.');
  const { id } = parseResponse(submission.stdout);
  await onSubmission(id);

  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const seconds = Math.min(waitTimeoutSeconds,
      Math.floor((remainingTime() - PROCESS_GRACE_MS) / 1000));
    if (seconds < 1) throw new Error('Notarization has insufficient deadline remaining.');
    const result = await execute(['wait', id, '--timeout', `${seconds}s`],
      seconds * 1000 + PROCESS_GRACE_MS);
    const hasOutput = String(result.stdout ?? '').trim().length > 0;
    const response = hasOutput ? parseResponse(result.stdout) : undefined;
    if (response && response.id.toLowerCase() !== id.toLowerCase()) {
      throw new Error('Notarization returned a different submission identifier.');
    }
    if (response && !['Accepted', 'In Progress', 'Invalid', 'Rejected'].includes(response.status)) {
      throw new Error('Notarization returned an unknown status.');
    }
    if (response?.status === 'Invalid' || response?.status === 'Rejected') {
      throw new Error(`Notarization status: ${response.status}.`);
    }
    if (response?.status === 'Accepted') {
      if (!processSucceeded(result)) throw new Error('Accepted status came from a failed process.');
      return id;
    }
    if (processSucceeded(result)) {
      if (response?.status !== 'In Progress') throw new Error('Notarization returned no status.');
    } else if (!isTransientFailure(result)) {
      throw new Error('Notarization wait failed with a nonrecoverable process error.');
    }
    if (attempt + 1 < attempts) {
      const pause = retryDelaySeconds * 1000;
      if (pause >= remainingTime()) throw new Error('Notarization deadline cannot accommodate retry.');
      await sleep(pause);
      remainingTime();
    }
  }
  throw new Error('Notarization did not finish within the wait attempt budget.');
}

async function main() {
  const [dmgPath, profile, keychainPath, ...extra] = process.argv.slice(2);
  if (extra.length || !dmgPath || !profile || !keychainPath) {
    throw new Error('Usage: notarize-dmg.mjs <file.dmg> <keychain-profile> <keychain-path>');
  }
  if (!/\.dmg$/i.test(dmgPath) || !(await stat(dmgPath)).isFile()) {
    throw new Error('The notarization input must be an existing DMG file.');
  }
  const id = await notarizeDmg({ dmgPath, profile, keychainPath }, {
    onSubmission: (submissionId) => console.log(`Notarization submission: ${submissionId}`),
  });
  console.log(id);
  console.log('Notarization status: Accepted');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(() => {
    console.error('Notarization failed; submission, wait status or input validation did not succeed.');
    process.exitCode = 1;
  });
}
