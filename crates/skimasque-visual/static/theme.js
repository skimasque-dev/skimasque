(() => {
  const key = 'skimasque-theme';
  const root = document.documentElement;
  let theme = 'system';
  try {
    const saved = localStorage.getItem(key);
    if (saved === 'light' || saved === 'dark') theme = saved;
  } catch (_) { /* Storage may be disabled; switching still works. */ }
  function apply() {
    if (theme === 'system') delete root.dataset.theme;
    else root.dataset.theme = theme;
  }
  apply();
  document.addEventListener('DOMContentLoaded', () => {
    const control = document.querySelector('.v-theme-control');
    const picker = document.querySelector('#site-theme');
    if (!control || !picker) return;
    picker.value = theme;
    control.hidden = false;
    picker.addEventListener('change', () => {
      theme = picker.value;
      apply();
      try { localStorage.setItem(key, theme); } catch (_) { /* Keep the current-page choice. */ }
    });
  });
})();
