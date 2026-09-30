const { test } = require('node:test');
const assert = require('node:assert/strict');
const { createPlayback } = require('../static/workflow.js');

function fixture() {
  const events = [];
  const timers = new Map();
  let id = 0;
  const playback = createPlayback({
    count: 6,
    onChange: state => events.push(state),
    schedule: fn => { timers.set(++id, fn); return id; },
    cancel: id => timers.delete(id),
  });
  return { playback, events, timers, advance() {
    const [id, fn] = timers.entries().next().value;
    timers.delete(id);
    fn();
  } };
}

test('play progresses through all six stages once and stops at expiry', () => {
  const f = fixture();
  assert.deepEqual(f.events[0], { step: 0, playing: false, finished: false });
  f.playback.play();
  f.playback.play();
  assert.equal(f.timers.size, 1);
  for (const expected of [1, 2, 3, 4, 5]) {
    f.advance();
    assert.equal(f.events.at(-1).step, expected);
  }
  assert.deepEqual(f.events.at(-1), { step: 5, playing: false, finished: true });
  f.playback.play();
  assert.equal(f.timers.size, 0);
});

test('pause cancels progression and play resumes from the same stage', () => {
  const f = fixture();
  f.playback.play();
  f.advance();
  f.playback.pause();
  assert.equal(f.timers.size, 0);
  assert.deepEqual(f.events.at(-1), { step: 1, playing: false, finished: false });
  f.playback.play();
  f.advance();
  assert.equal(f.events.at(-1).step, 2);
});

test('replay resets an in-progress or finished sequence without duplicate timers', () => {
  const f = fixture();
  f.playback.play();
  f.advance();
  f.playback.replay();
  assert.deepEqual(f.events.at(-1), { step: 0, playing: true, finished: false });
  assert.equal(f.timers.size, 1);
  for (let i = 0; i < 5; i++) f.advance();
  f.playback.replay();
  assert.deepEqual(f.events.at(-1), { step: 0, playing: true, finished: false });
  assert.equal(f.timers.size, 1);
});
