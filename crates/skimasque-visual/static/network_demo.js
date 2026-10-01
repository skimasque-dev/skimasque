(() => {
  const root = document.currentScript.previousElementSibling;
  const motion = matchMedia('(prefers-reduced-motion: reduce)');
  const controls = root.querySelector('.v-network-controls');
  const pause = controls.querySelector('[data-action="pause"]');
  const caption = root.querySelector('.v-network-caption');
  const captions = [
    'Workload → control plane: request access to db.prod:5432.',
    'Control plane → workload: approved; temporary session issued.',
    'Control plane → gateway: authorize the scoped session.',
    'Gateway → control plane: session authorization acknowledged.',
    'Workload → gateway → database: send the deployment traffic.',
    'Database → gateway → workload: return the response.',
    'Session expired. Access is closed; other services stayed blocked.',
  ];
  let step = 0, timer = null, playing = false, started = false;
  function render() {
    root.dataset.step = String(step);
    root.dataset.playing = String(playing);
    caption.textContent = captions[step];
    pause.textContent = playing ? 'Pause' : 'Resume';
    pause.disabled = step === captions.length - 1;
  }
  function stop() { clearTimeout(timer); timer = null; playing = false; }
  function play() {
    if (motion.matches || playing || step === captions.length - 1 || document.hidden) return;
    playing = true; render();
    timer = setTimeout(() => { playing = false; step++; render(); play(); }, step === 4 || step === 5 ? 4000 : 2500);
  }
  pause.addEventListener('click', () => { if (playing) { stop(); render(); } else play(); });
  controls.querySelector('[data-action="replay"]').addEventListener('click', () => { stop(); step = 0; play(); });
  function updateMotion() {
    stop(); controls.hidden = motion.matches;
    if (motion.matches) {
      delete root.dataset.step; delete root.dataset.playing;
      caption.textContent = 'Only the permitted database is reachable. Access closes when the session expires.';
    } else { render(); }
  }
  motion.addEventListener('change', updateMotion);
  document.addEventListener('visibilitychange', () => { if (document.hidden) { stop(); if (!motion.matches) render(); } });
  updateMotion();
  if ('IntersectionObserver' in window) {
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting) && !started && !motion.matches) {
        started = true; play(); observer.disconnect();
      }
    }, { threshold: 0.3 });
    observer.observe(root);
  } else { started = true; play(); }
})();
