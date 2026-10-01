const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const script = () => fs.readFileSync(require('node:path').join(__dirname, '../static/theme.js'), 'utf8');
function page(saved, blocked = false) {
  const root = { dataset: {} };
  const picker = { value: '', addEventListener(_, fn) { this.change = fn; } };
  const control = { hidden: true };
  let ready, stored = saved;
  vm.runInNewContext(script(), {
    document: { documentElement: root, addEventListener(_, fn) { ready = fn; },
      querySelector(selector) { return selector === '.v-theme-control' ? control : picker; } },
    localStorage: { getItem() { if (blocked) throw Error(); return stored; },
      setItem(_, value) { if (blocked) throw Error(); stored = value; } },
  });
  return { root, picker, control, ready: () => ready(), stored: () => stored };
}
test('restores explicit theme before controls initialize; System removes override', () => {
  const p = page('dark');
  assert.equal(p.root.dataset.theme, 'dark');
  p.ready();
  assert.equal(p.picker.value, 'dark');
  assert.equal(p.control.hidden, false);
  p.picker.value = 'system'; p.picker.change();
  assert.equal(p.root.dataset.theme, undefined);
  assert.equal(p.stored(), 'system');
});
test('unavailable storage still allows theme switching', () => {
  const p = page(null, true); p.ready();
  assert.equal(p.picker.value, 'system');
  p.picker.value = 'light'; p.picker.change();
  assert.equal(p.root.dataset.theme, 'light');
});
