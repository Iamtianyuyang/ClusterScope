const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');

export function pageTransition(view, direction = 1) {
  if (!view || motionQuery.matches || !view.animate) return;
  view.animate([
    { opacity: 0, transform: `translateY(${direction * 4}px)` },
    { opacity: 1, transform: 'translateY(0)' },
  ], { duration: 160, easing: 'cubic-bezier(.23, 1, .32, 1)' });
}

export function spinRefresh(button) {
  if (!button || motionQuery.matches) return;
  button.querySelector('svg')?.animate(
    [{ transform: 'rotate(0)' }, { transform: 'rotate(360deg)' }],
    { duration: 420, easing: 'cubic-bezier(.23, 1, .32, 1)' },
  );
}

export function createCountdown(element, getSeconds) {
  if (!element) return { reset() {} };
  const update = () => {
    const seconds = Math.max(0, Math.ceil(getSeconds()));
    element.textContent = String(seconds);
    element.parentElement?.setAttribute('title', `下次自动刷新 ${seconds} 秒`);
  };
  update();
  const timer = setInterval(update, 250);
  return {
    reset() { update(); },
    stop() { clearInterval(timer); },
  };
}

/**
 * Repaint a flap cell in place. The text changes once; a 180ms turn masks the
 * swap. Only the focused row and the summary call this, so a 3s refresh never
 * turns the whole board into noise.
 */
export function flipCell(cell, text, tone = '', animate = true) {
  if (!cell) return;
  const target = cell.querySelector('.flap-face') || cell;
  const next = String(text);
  const changed = cell.dataset.value !== next;
  cell.dataset.value = next;
  if (changed) {
    target.textContent = next;
    if (animate && !motionQuery.matches && !document.hidden) {
      cell.classList.remove('flipping');
      void cell.offsetWidth;
      cell.classList.add('flipping');
      clearTimeout(cell._flipTimer);
      cell._flipTimer = setTimeout(() => cell.classList.remove('flipping'), 200);
    }
  }
  if (tone !== undefined) {
    cell.classList.remove('hot', 'critical', 'idle');
    if (tone) cell.classList.add(tone);
  }
}
