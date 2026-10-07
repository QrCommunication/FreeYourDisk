import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawnSync } from 'node:child_process';
import { chmod, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { notarizeDmg } from './notarize-dmg.mjs';

const ID = '12345678-1234-1234-1234-123456789abc';
const OTHER_ID = '22345678-1234-1234-1234-123456789abc';
const CONFIG = {
  dmgPath: '/tmp/Free Your Disk.dmg',
  profile: 'release-profile',
  keychainPath: '/tmp/signing.keychain-db',
};

function response(body, status = 0, stderr = '') {
  return { status, stdout: JSON.stringify(body), stderr };
}

function harness(results, overrides = {}) {
  const calls = [];
  const pauses = [];
  let elapsed = 0;
  const options = {
    run(file, args, processOptions) {
      calls.push({ file, args, options: processOptions });
      assert.ok(results.length, 'Unexpected extra external process');
      const next = results.shift();
      return typeof next === 'function' ? next() : next;
    },
    async sleep(ms) {
      pauses.push(ms);
      elapsed += ms;
    },
    now: () => elapsed,
    attempts: 3,
    waitTimeoutSeconds: 2,
    retryDelaySeconds: 1,
    ...overrides,
  };
  return { calls, pauses, options, advance: (ms) => { elapsed += ms; } };
}

function assertSingleSubmission(calls) {
  assert.equal(calls.filter(({ args }) => args[1] === 'submit').length, 1);
  for (const { file, args } of calls) {
    assert.equal(file, 'xcrun');
    assert.equal(args[0], 'notarytool');
    assert.equal(args[args.indexOf('--keychain-profile') + 1], CONFIG.profile);
    assert.equal(args[args.indexOf('--keychain') + 1], CONFIG.keychainPath);
    assert.equal(args[args.indexOf('--output-format') + 1], 'json');
    if (args[1] === 'wait') assert.equal(args[2], ID);
  }
}

test('submits once without --wait, then waits for an Accepted response', async () => {
  const h = harness([response({ id: ID }), response({ id: ID, status: 'Accepted' })]);
  assert.equal(await notarizeDmg(CONFIG, h.options), ID);
  assertSingleSubmission(h.calls);
  assert.equal(h.calls[0].args[2], CONFIG.dmgPath);
  assert.ok(!h.calls[0].args.includes('--wait'));
  assert.equal(h.calls.length, 2);
  assert.deepEqual(h.pauses, []);
});

for (const failure of [
  { name: 'network interruption -1005', result: { status: 1, stdout: '', stderr: 'NSURLErrorDomain Code=-1005 The network connection was lost.' } },
  { name: 'process timeout 124', result: { status: 124, stdout: '', stderr: 'Timed out' } },
  { name: 'spawn timeout', result: { status: null, stdout: '', stderr: '', error: Object.assign(new Error('timeout'), { code: 'ETIMEDOUT' }) } },
  { name: 'In Progress JSON', result: response({ id: ID, status: 'In Progress' }) },
]) {
  test(`resumes the same ID after ${failure.name}, never resubmitting`, async () => {
    const h = harness([response({ id: ID }), failure.result, response({ id: ID, status: 'Accepted' })]);
    assert.equal(await notarizeDmg(CONFIG, h.options), ID);
    assertSingleSubmission(h.calls);
    assert.equal(h.calls.length, 3);
    assert.deepEqual(h.pauses, [1000]);
  });
}

for (const status of ['Invalid', 'Rejected']) {
  test(`${status} is a terminal failure, without retries`, async () => {
    const h = harness([response({ id: ID }), response({ id: ID, status })]);
    await assert.rejects(notarizeDmg(CONFIG, h.options));
    assert.equal(h.calls.length, 2);
    assert.deepEqual(h.pauses, []);
  });
}

for (const [name, result] of [
  ['malformed JSON', { status: 0, stdout: '{broken', stderr: '' }],
  ['missing identifier', response({ status: 'Accepted' })],
  ['mismatched identifier', response({ id: OTHER_ID, status: 'Accepted' })],
  ['unknown status', response({ id: ID, status: 'Something Else' })],
  ['empty successful stdout', { status: 0, stdout: '', stderr: '' }],
  ['nonzero Accepted output', response({ id: ID, status: 'Accepted' }, 1)],
]) {
  test(`fails closed for ${name}`, async () => {
    const h = harness([response({ id: ID }), result]);
    await assert.rejects(notarizeDmg(CONFIG, h.options));
    assert.equal(h.calls.length, 2);
    assert.deepEqual(h.pauses, []);
  });
}

for (const [name, result] of [
  ['nonzero submission', { status: 1, stdout: '', stderr: 'Upload failed' }],
  ['missing submission ID', response({})],
  ['invalid submission ID', response({ id: '--help' })],
  ['malformed submission JSON', { status: 0, stdout: 'not json', stderr: '' }],
]) {
  test(`${name} never starts polling or retries an upload`, async () => {
    const h = harness([result]);
    await assert.rejects(notarizeDmg(CONFIG, h.options));
    assert.equal(h.calls.length, 1);
    assert.deepEqual(h.pauses, []);
  });
}

test('fails after its finite attempt budget, without sleeping after the final attempt', async () => {
  const h = harness([response({ id: ID }), ...Array.from({ length: 3 }, () => response({ id: ID, status: 'In Progress' }))]);
  await assert.rejects(notarizeDmg(CONFIG, h.options));
  assertSingleSubmission(h.calls);
  assert.equal(h.calls.length, 4);
  assert.deepEqual(h.pauses, [1000, 1000]);
});

test('authentication 401 fails immediately rather than exhausting the retry budget', async () => {
  const h = harness([response({ id: ID }), { status: 1, stdout: '', stderr: 'HTTP status code: 401. Invalid credentials.' }]);
  await assert.rejects(notarizeDmg(CONFIG, h.options));
  assert.equal(h.calls.length, 2);
  assert.deepEqual(h.pauses, []);
});

test('unknown process errors fail immediately', async () => {
  const h = harness([response({ id: ID }), { status: 1, stdout: '', stderr: 'Unexpected service failure' }]);
  await assert.rejects(notarizeDmg(CONFIG, h.options));
  assert.equal(h.calls.length, 2);
  assert.deepEqual(h.pauses, []);
});

for (const field of ['dmgPath', 'profile', 'keychainPath']) {
  for (const invalid of ['', '   ', null]) {
    test(`rejects invalid ${field} before calling any external process (${JSON.stringify(invalid)})`, async () => {
      const h = harness([]);
      await assert.rejects(notarizeDmg({ ...CONFIG, [field]: invalid }, h.options));
      assert.equal(h.calls.length, 0);
    });
  }
}

for (const [field, invalid] of [
  ['attempts', 0], ['attempts', 13], ['attempts', 1.5],
  ['waitTimeoutSeconds', 0], ['waitTimeoutSeconds', 601], ['waitTimeoutSeconds', NaN],
  ['retryDelaySeconds', -1], ['retryDelaySeconds', 61], ['retryDelaySeconds', Infinity],
]) {
  test(`rejects invalid ${field}=${invalid} before external calls`, async () => {
    const h = harness([], { [field]: invalid });
    await assert.rejects(notarizeDmg(CONFIG, h.options));
    assert.equal(h.calls.length, 0);
  });
}

test('zero retry delay and minimum wait timeout are accepted', async () => {
  const h = harness([response({ id: ID }), response({ id: ID, status: 'In Progress' }), response({ id: ID, status: 'Accepted' })], { retryDelaySeconds: 0, waitTimeoutSeconds: 1 });
  assert.equal(await notarizeDmg(CONFIG, h.options), ID);
  assert.equal(h.calls[1].options.timeout, 31000);
  assert.ok(h.pauses.every((ms) => ms === 0));
});

test('defaults bound every process and enforce hard process termination', async () => {
  const h = harness([response({ id: ID }), response({ id: ID, status: 'Accepted' })]);
  delete h.options.attempts;
  delete h.options.waitTimeoutSeconds;
  delete h.options.retryDelaySeconds;
  assert.equal(await notarizeDmg(CONFIG, h.options), ID);
  assert.equal(h.calls[0].options.timeout, 300000);
  assert.equal(h.calls[1].options.timeout, 630000);
  for (const { options } of h.calls) assert.equal(options.killSignal, 'SIGKILL');
});

test('global 120-minute deadline includes time spent in external processes', async () => {
  const h = harness([response({ id: ID }), () => {
    h.advance(120 * 60 * 1000);
    return response({ id: ID, status: 'In Progress' });
  }]);
  await assert.rejects(notarizeDmg(CONFIG, h.options));
  assert.equal(h.calls.length, 2);
  assert.deepEqual(h.pauses, []);
});

for (const dependency of ['run', 'sleep', 'now', 'onSubmission']) {
  test(`rejects a nonfunction ${dependency} dependency`, async () => {
    const h = harness([], { [dependency]: null });
    await assert.rejects(notarizeDmg(CONFIG, h.options));
    assert.equal(h.calls.length, 0);
  });
}

test('missing process result cannot be treated as success', async () => {
  const h = harness([undefined]);
  await assert.rejects(notarizeDmg(CONFIG, h.options));
  assert.equal(h.calls.length, 1);
});

test('invalid clock rejects before running a process', async () => {
  const h = harness([], { now: () => NaN });
  await assert.rejects(notarizeDmg(CONFIG, h.options));
  assert.equal(h.calls.length, 0);
});

test('does not start a wait if less than the process grace period remains', async () => {
  const h = harness([() => {
    h.advance(120 * 60 * 1000 - 20000);
    return response({ id: ID });
  }]);
  await assert.rejects(notarizeDmg(CONFIG, h.options));
  assert.equal(h.calls.length, 1);
});

test('does not sleep past the deadline before a retry', async () => {
  const h = harness([response({ id: ID }), () => {
    h.advance(120 * 60 * 1000 - 500);
    return response({ id: ID, status: 'In Progress' });
  }]);
  await assert.rejects(notarizeDmg(CONFIG, h.options));
  assert.equal(h.calls.length, 2);
  assert.deepEqual(h.pauses, []);
});

test('reports the submission ID exactly once, before any wait even after a network retry', async () => {
  const events = [];
  const h = harness([
    () => { events.push('submit'); return response({ id: ID }); },
    () => { events.push('wait'); return { status: 1, stdout: '', stderr: 'NSURLErrorDomain Code=-1005' }; },
    () => { events.push('wait'); return response({ id: ID, status: 'Accepted' }); },
  ], { onSubmission: (id) => events.push(id) });
  assert.equal(await notarizeDmg(CONFIG, h.options), ID);
  assert.deepEqual(events, ['submit', ID, 'wait', 'wait']);
});

for (const [name, result] of [
  ['terminal Invalid', response({ id: ID, status: 'Invalid' })],
  ['unrecoverable network failure', { status: 1, stdout: '', stderr: 'unexpected network failure' }],
]) {
  test(`keeps the submission observable after ${name}`, async () => {
    const ids = [];
    const h = harness([response({ id: ID }), result], { onSubmission: (id) => ids.push(id) });
    await assert.rejects(notarizeDmg(CONFIG, h.options));
    assert.deepEqual(ids, [ID]);
  });
}

test('a failing submission observer does not begin waiting', async () => {
  const h = harness([response({ id: ID })], { onSubmission: () => { throw new Error('Cannot persist submission'); } });
  await assert.rejects(notarizeDmg(CONFIG, h.options), /Cannot persist submission/);
  assert.equal(h.calls.length, 1);
});

async function cliFixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'fyd-notary-test-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const dmg = join(directory, 'Free Your Disk.dmg');
  const executable = join(directory, 'fake-xcrun.mjs');
  const trace = join(directory, 'calls.jsonl');
  await writeFile(dmg, 'Disposable DMG fixture, never submitted to Apple');
  await writeFile(executable, `#!/usr/bin/env node
import { appendFileSync } from 'node:fs';
appendFileSync(process.env.FYD_TEST_TRACE, JSON.stringify(process.argv.slice(2)) + '\\n');
if (process.env.FYD_TEST_FAIL || (process.env.FYD_TEST_WAIT_FAIL && process.argv[3] === 'wait')) {
  console.error('FAKE_SECRET_MUST_NOT_ESCAPE');
  process.exit(1);
}
console.log(JSON.stringify(process.argv[3] === 'submit'
  ? { id: '${ID}' } : { id: '${ID}', status: 'Accepted' }));
`);
  await chmod(executable, 0o700);
  const invoke = (args = [dmg, CONFIG.profile, CONFIG.keychainPath], env = {}) => spawnSync(
    process.execPath,
    [new URL('./notarize-dmg.mjs', import.meta.url).pathname, ...args],
    { encoding: 'utf8', timeout: 10000, env: { ...process.env, XCRUN_BIN: executable, FYD_TEST_TRACE: trace, ...env } },
  );
  return { directory, dmg, trace, invoke };
}

test('CLI validates its input and accepts only the completed matching submission', async (t) => {
  const fixture = await cliFixture(t);
  const result = fixture.invoke();
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, new RegExp(`^Notarization submission: ${ID}\\n`));
  assert.match(result.stdout, /Notarization status: Accepted\n$/);
  const calls = (await readFile(fixture.trace, 'utf8')).trim().split('\n').map(JSON.parse);
  assert.equal(calls.length, 2);
  assert.equal(calls[0][1], 'submit');
  assert.equal(calls[0][2], fixture.dmg);
  assert.equal(calls[1][1], 'wait');
  assert.equal(calls[1][2], ID);
});

test('CLI failure output never leaks subprocess diagnostics or credentials', async (t) => {
  const fixture = await cliFixture(t);
  const result = fixture.invoke(undefined, { FYD_TEST_FAIL: '1' });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  assert.match(result.stderr, /Notarization failed/);
  assert.ok(!result.stderr.includes('FAKE_SECRET_MUST_NOT_ESCAPE'));
});

test('CLI preserves the initial ID when a later wait fails, without leaking its diagnostics', async (t) => {
  const fixture = await cliFixture(t);
  const result = fixture.invoke(undefined, { FYD_TEST_WAIT_FAIL: '1' });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, `Notarization submission: ${ID}\n`);
  assert.match(result.stderr, /Notarization failed/);
  assert.ok(!result.stderr.includes('FAKE_SECRET_MUST_NOT_ESCAPE'));
});

test('CLI rejects missing arguments, nonexistent paths, directories and non-DMG files before spawning', async (t) => {
  const fixture = await cliFixture(t);
  const notDmg = join(fixture.directory, 'application.zip');
  await writeFile(notDmg, 'not a DMG');
  for (const args of [[], [fixture.dmg, CONFIG.profile, CONFIG.keychainPath, 'extra'],
    [join(fixture.directory, 'missing.dmg'), CONFIG.profile, CONFIG.keychainPath],
    [fixture.directory, CONFIG.profile, CONFIG.keychainPath],
    [notDmg, CONFIG.profile, CONFIG.keychainPath]]) {
    const result = fixture.invoke(args);
    assert.equal(result.status, 1);
    assert.equal(result.stdout, '');
  }
  await assert.rejects(readFile(fixture.trace), { code: 'ENOENT' });
});
