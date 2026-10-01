const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
test('network playback visits requests, approvals, acknowledgements, traffic and responses before expiry', () => {
  const buttons = {};
  const button = name => buttons[name] ||= { addEventListener(_, fn) { this.click = fn; } };
  const caption = {};
  const controls = { querySelector(s) { return button(s); } };
  const root = { dataset: {}, querySelector(s) { return s === '.v-network-controls' ? controls : caption; } };
  let pending = null;
  vm.runInNewContext(fs.readFileSync(require('node:path').join(__dirname, '../static/network_demo.js'), 'utf8'), {
    document: { currentScript: { previousElementSibling: root }, hidden: false, addEventListener() {} },
    window: {}, matchMedia: () => ({ matches: false, addEventListener() {} }),
    setTimeout(fn) { pending = fn; return 1; }, clearTimeout() { pending = null; },
  });
  const seen = [caption.textContent];
  for (let i = 0; i < 6; i++) { const tick = pending; pending = null; tick(); seen.push(caption.textContent); }
  assert.deepEqual(seen.map(s => s.split(':')[0]), [
    'Workload → control plane', 'Control plane → workload', 'Control plane → gateway',
    'Gateway → control plane', 'Workload → gateway → database', 'Database → gateway → workload',
    'Session expired. Access is closed; other services stayed blocked.',
  ]);
  assert.equal(pending, null);
  assert.equal(root.dataset.playing, 'false');
  buttons['[data-action="replay"]'].click();
  assert.equal(root.dataset.step, '0');
  buttons['[data-action="pause"]'].click();
  assert.equal(pending, null);
  assert.equal(root.dataset.playing, 'false');
});
