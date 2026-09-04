import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

const outputs = [
  'windows/host/src/protocol/generated.rs',
  'ipad/SidecarDOS/Protocol/Generated.swift',
];

function fixture(t) {
  const tempRoot = fs.realpathSync(os.tmpdir());
  const root = fs.mkdtempSync(path.join(tempRoot, 'sidecardos-protocol-'));
  t.after(() => {
    assert.equal(path.dirname(root), tempRoot);
    assert.ok(path.basename(root).startsWith('sidecardos-protocol-'));
    fs.rmSync(root, { recursive: true, force: true });
  });
  fs.mkdirSync(path.join(root, 'protocol'));
  for (const name of ['generate.mjs', 'schema.json']) {
    fs.copyFileSync(new URL(name, import.meta.url), path.join(root, 'protocol', name));
  }
  const run = (...args) => spawnSync(process.execPath, [path.join(root, 'protocol/generate.mjs'), ...args], {
    cwd: root,
    encoding: 'utf8',
  });
  const generated = run();
  assert.equal(generated.status, 0, generated.stderr);
  return { root, run };
}

test('fresh LF output passes the compatibility check', t => {
  const { root, run } = fixture(t);
  for (const output of outputs) {
    assert.ok(!fs.readFileSync(path.join(root, output), 'utf8').includes('\r'));
  }
  const result = run('--check');
  assert.equal(result.status, 0, result.stderr);
});

for (const output of outputs) {
  test(`Windows CRLF checkout passes without rewriting ${output}`, t => {
    const { root, run } = fixture(t);
    const file = path.join(root, output);
    const crlf = fs.readFileSync(file, 'utf8').replace(/\n/g, '\r\n');
    fs.writeFileSync(file, crlf);
    const result = run('--check');
    assert.equal(result.status, 0, result.stderr);
    assert.equal(fs.readFileSync(file, 'utf8'), crlf);
  });

  test(`real protocol drift is rejected in ${output}`, t => {
    const { root, run } = fixture(t);
    const file = path.join(root, output);
    const stale = fs.readFileSync(file, 'utf8').replace(/UInt8|u8/, 'INVALID_WIRE_TYPE').replace(/\n/g, '\r\n');
    fs.writeFileSync(file, stale);
    const result = run('--check');
    assert.notEqual(result.status, 0);
    assert.ok(result.stderr.includes(`${output} is stale`), result.stderr);
    assert.equal(fs.readFileSync(file, 'utf8'), stale);
  });

  test(`missing generated file is rejected: ${output}`, t => {
    const { root, run } = fixture(t);
    fs.unlinkSync(path.join(root, output));
    const result = run('--check');
    assert.notEqual(result.status, 0);
    assert.ok(result.stderr.includes(`${output} is stale`), result.stderr);
  });
}
