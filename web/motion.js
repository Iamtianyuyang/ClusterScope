const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
const clamp = (value, min = 0, max = 1) => Math.min(max, Math.max(min, value));
const progress = (time, start, end) => clamp((time - start) / (end - start));
const mod = (value, divisor) => ((value % divisor) + divisor) % divisor;

export function pageTransition(view, direction = 1) {
  if (!view || motionQuery.matches || !view.animate) return;
  view.animate([
    { opacity: 0, transform: `translateY(${direction * 6}px)` },
    { opacity: 1, transform: 'translateY(0)' },
  ], { duration: 200, easing: 'cubic-bezier(0, 0, .2, 1)' });
}

// Motion Prompt #084 · MIT License (c) 2026 Shohei Ohtaki · https://showheyohtaki.github.io/motion-prompt/#084
const elasticOut = value => value <= 0 ? 0 : value >= 1 ? 1
  : 2 ** (-10 * value) * Math.sin((value - .075) * 2 * Math.PI / .3) + 1;

export function initMagneticButtons() {
  if (motionQuery.matches || !window.matchMedia('(pointer: fine)').matches) return;
  const states = new WeakMap();
  let pointer = { x: -1000, y: -1000 };
  const update = () => {
    document.querySelectorAll('.magnetic').forEach(button => {
      const rect = button.getBoundingClientRect();
      const center = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
      const distance = Math.hypot(pointer.x - center.x, pointer.y - center.y);
      let state = states.get(button) || { x: 0, y: 0, inside: false, leftAt: 0, fromX: 0, fromY: 0 };
      if (distance < 110) {
        if (!state.inside) { state.inside = true; }
        state.x += ((pointer.x - center.x) * .35 - state.x) * .18;
        state.y += ((pointer.y - center.y) * .35 - state.y) * .18;
      } else {
        if (state.inside) {
          Object.assign(state, { inside: false, leftAt: performance.now(), fromX: state.x, fromY: state.y });
        }
        const factor = 1 - elasticOut(progress((performance.now() - state.leftAt) / 1000, 0, 1.1));
        state.x = state.fromX * factor;
        state.y = state.fromY * factor;
      }
      button.style.transform = `translate(${state.x}px, ${state.y}px)`;
      states.set(button, state);
    });
  };
  document.addEventListener('pointermove', event => {
    pointer = { x: event.clientX, y: event.clientY };
  }, { passive: true });
  document.addEventListener('pointerleave', () => { pointer = { x: -1000, y: -1000 }; });
  const frame = () => { if (!document.hidden) update(); requestAnimationFrame(frame); };
  requestAnimationFrame(frame);
}

// Motion Prompt #121-144 · MIT License (c) 2026 Shohei Ohtaki · https://showheyohtaki.github.io/motion-prompt/#121
// Shared motion2 helpers: deterministic time and seeded offsets.
const snapTime = value => mod(Math.round(value * 1e6) / 1e6, 4.7);
function hash01(value) {
  value = Math.imul((value | 0) ^ 0x9e3779b9, 0x85ebca6b);
  value ^= value >>> 13;
  value = Math.imul(value, 0xc2b2ae35);
  value ^= value >>> 16;
  return (value >>> 0) / 4294967296;
}

function densify(points, closed, step) {
  const output = [];
  const count = closed ? points.length : points.length - 1;
  for (let i = 0; i < count; i += 1) {
    const a = points[i], b = points[(i + 1) % points.length];
    const pieces = Math.max(1, Math.round(Math.hypot(b[0] - a[0], b[1] - a[1]) / step));
    for (let j = 0; j < pieces; j += 1) output.push([a[0] + (b[0] - a[0]) * j / pieces, a[1] + (b[1] - a[1]) * j / pieces]);
  }
  if (!closed) output.push(points.at(-1));
  return output;
}

// Motion Prompt #136 · MIT License (c) 2026 Shohei Ohtaki · https://showheyohtaki.github.io/motion-prompt/#136
export function startLineBoil(canvas) {
  if (!canvas) return;
  const context = canvas.getContext('2d');
  const star = [];
  for (let i = 0; i < 10; i += 1) {
    const radius = i % 2 ? 30 : 70, angle = -Math.PI / 2 + i * Math.PI / 5;
    star.push([150 + Math.cos(angle) * radius, 112 + Math.sin(angle) * radius]);
  }
  const cloud = [];
  for (let i = 0; i < 96; i += 1) {
    const angle = i / 96 * Math.PI * 2;
    cloud.push([388 + Math.cos(angle) * (80 + 18 * Math.sin(3 * angle)), 126 + Math.sin(angle) * (34 + 8 * Math.cos(3 * angle))]);
  }
  const shapes = [
    { points: densify(star, true, 13), color: '#ffd54a', closed: true, width: 8 },
    { points: cloud, color: '#9bb4c0', closed: true, width: 8 },
    { points: densify([[248, 44], [266, 62]], false, 6), color: '#00deec', closed: false, width: 7 },
    { points: densify([[254, 104], [280, 104]], false, 6), color: '#00deec', closed: false, width: 7 },
    { points: densify([[60, 34], [74, 52]], false, 6), color: '#00deec', closed: false, width: 7 },
    { points: densify([[318, 178], [460, 178]], false, 10), color: '#1b4154', closed: false, width: 6 },
  ];
  const startedAt = performance.now();
  const draw = now => {
    const time = motionQuery.matches ? 0 : snapTime((now - startedAt) / 1000);
    const frameIndex = Math.floor(time / (4.7 / 47));
    context.clearRect(0, 0, 539, 208); context.lineCap = 'round'; context.lineJoin = 'round';
    shapes.forEach((shape, shapeIndex) => {
      const points = shape.points.map((point, index) => [
        point[0] + (hash01(frameIndex * 7919 + index * 131 + shapeIndex * 17) - .5) * 7,
        point[1] + (hash01(frameIndex * 6007 + index * 97 + shapeIndex * 29 + 5) - .5) * 7,
      ]);
      context.strokeStyle = shape.color; context.lineWidth = shape.width; context.beginPath();
      const middle = (a, b) => [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
      if (shape.closed) {
        const first = middle(points.at(-1), points[0]); context.moveTo(...first);
        points.forEach((point, index) => { const mid = middle(point, points[(index + 1) % points.length]); context.quadraticCurveTo(...point, ...mid); });
        context.closePath();
      } else {
        context.moveTo(...points[0]);
        for (let index = 1; index < points.length - 1; index += 1) {
          const mid = middle(points[index], points[index + 1]); context.quadraticCurveTo(...points[index], ...mid);
        }
        context.lineTo(...points.at(-1));
      }
      context.stroke();
    });
  };
  draw(startedAt);
  if (motionQuery.matches) return;
  const frame = now => {
    if (!canvas.isConnected) return;
    if (!document.hidden) draw(now);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}

// Motion Prompt #164 · MIT License (c) 2026 Shohei Ohtaki · https://showheyohtaki.github.io/motion-prompt/#164
export function createCountdown(element, getSeconds) {
  if (!element) return { reset() {} };
  const digits = [];
  for (let index = 0; index < 8; index += 1) {
    if (index === 2 || index === 5) {
      const separator = document.createElement('span'); separator.className = 'countdown-separator'; separator.textContent = ':'; element.append(separator);
    } else {
      const digit = document.createElement('span'); digit.className = 'countdown-digit';
      digit.dataset.group = digits.length < 2 ? 'hours' : digits.length < 4 ? 'minutes' : 'seconds';
      const value = document.createElement('span'); value.textContent = '0'; digit.append(value); element.append(digit); digits.push(digit);
    }
  }
  let previous = '';
  const update = () => {
    const seconds = Math.max(0, Math.ceil(getSeconds()));
    const hours = Math.floor(seconds / 3600), minutes = Math.floor(seconds / 60) % 60, secs = seconds % 60;
    const next = [hours, minutes, secs].map(value => String(value).padStart(2, '0')).join('');
    element.setAttribute('aria-label', `距下次刷新 ${seconds} 秒`);
    [...next].forEach((value, index) => {
      if (previous[index] === value) return;
      const tile = digits[index];
      const old = tile.firstElementChild;
      if (!motionQuery.matches && old?.animate && previous) {
        const incoming = document.createElement('span'); incoming.textContent = value; tile.append(incoming);
        old.animate([{ transform: 'translateY(0)' }, { transform: 'translateY(-24px)' }], { duration: 320, easing: 'cubic-bezier(.42,0,.58,1)' }).finished.then(() => old.remove()).catch(() => old.remove());
        incoming.animate([{ transform: 'translateY(24px)' }, { transform: 'translateY(0)' }], { duration: 320, easing: 'cubic-bezier(.42,0,.58,1)' });
      } else { old.textContent = value; }
    });
    previous = next;
  };
  update();
  setInterval(update, 200);
  return { reset() { previous = ''; update(); } };
}

// Motion Prompt #024 · MIT License (c) 2026 Shohei Ohtaki · https://showheyohtaki.github.io/motion-prompt/#024
export function impactStatus(target) {
  if (!target || motionQuery.matches || !target.animate) return;
  target.animate([
    { transform: 'translateY(-120px) scaleY(1.12)', offset: 0 },
    { transform: 'translateY(-120px) scaleY(1.12)', offset: .3 / 2.2, easing: 'cubic-bezier(.55,0,1,.45)' },
    { transform: 'translateY(0) scaleY(1.12)', offset: .62 / 2.2 },
    { transform: 'translateY(0) scale(1.22,.78)', offset: .77 / 2.2 },
    { transform: 'translateY(0) scale(1)', offset: .92 / 2.2 },
    { transform: 'translateY(0) scale(1)', offset: 1 },
  ], { duration: 2200 });
  const frames = [];
  for (let index = 0; index <= 66; index += 1) {
    const time = index / 60;
    const envelope = Math.exp(-5 * time), phase = 2 * Math.PI * 5 * time;
    frames.push({ transform: `translate(${15 * envelope * Math.sin(phase)}px, ${11 * envelope * Math.cos(phase * 1.3)}px) rotate(${.02 * envelope * Math.sin(phase + .6)}rad)` });
  }
  document.querySelector('main')?.animate(frames, { duration: 1100, delay: 620 });
}
