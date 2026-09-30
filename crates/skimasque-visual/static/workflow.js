(() => {
  function createPlayback({ count, onChange, schedule = setTimeout, cancel = clearTimeout }) {
    let step = 0;
    let playing = false;
    let timer = null;
    const emit = () => onChange({ step, playing, finished: step === count - 1 });
    const stop = () => {
      if (timer !== null) cancel(timer);
      timer = null;
      playing = false;
    };
    const tick = () => {
      timer = schedule(() => {
        timer = null;
        if (!playing) return;
        step += 1;
        if (step === count - 1) playing = false;
        emit();
        if (playing) tick();
      }, 2500);
    };
    const play = () => {
      if (playing || step === count - 1) return;
      playing = true;
      emit();
      tick();
    };
    emit();
    return {
      play,
      pause() { stop(); emit(); },
      replay() { stop(); step = 0; play(); },
    };
  }

  if (typeof module !== 'undefined' && module.exports) module.exports = { createPlayback };
  if (typeof document === 'undefined') return;
  const root = document.currentScript.previousElementSibling;
  if (!root || !root.classList.contains('v-workflow')) return;
  const stages = [...root.querySelectorAll('.v-workflow-stage')];
  const controls = root.querySelector('.v-workflow-controls');
  const progress = root.querySelector('.v-workflow-progress');
  const play = controls.querySelector('[data-action="play"]');
  const pause = controls.querySelector('[data-action="pause"]');
  const replay = controls.querySelector('[data-action="replay"]');
  const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const playback = createPlayback({
    count: stages.length,
    onChange({ step, playing, finished }) {
      root.dataset.playing = String(playing && !motion.matches);
      stages.forEach((stage, index) => {
        stage.dataset.current = String(index === step && !motion.matches);
        stage.dataset.complete = String(index < step && !motion.matches);
        if (index === step && !motion.matches) stage.setAttribute('aria-current', 'step');
        else stage.removeAttribute('aria-current');
      });
      play.disabled = playing || finished;
      pause.disabled = !playing;
      if (!motion.matches) {
        const title = stages[step].querySelector('strong').textContent;
        progress.textContent = `Step ${step + 1} of ${stages.length}: ${title}${finished ? ' — complete.' : playing ? '.' : ' — paused.'}`;
      }
    },
  });
  function updateMotion() {
    controls.hidden = motion.matches;
    playback.pause();
    if (motion.matches) progress.textContent = 'Reduced motion is enabled. All six steps are shown as a static example.';
  }
  play.addEventListener('click', () => { if (!motion.matches) playback.play(); });
  pause.addEventListener('click', () => playback.pause());
  replay.addEventListener('click', () => { if (!motion.matches) playback.replay(); });
  motion.addEventListener('change', updateMotion);
  document.addEventListener('visibilitychange', () => { if (document.hidden) playback.pause(); });
  updateMotion();
})();
