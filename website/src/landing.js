// @ts-check

/** @type {HTMLElement | null} */
const year = document.querySelector("[data-current-year]");
if (year) year.textContent = String(new Date().getFullYear());

// A restrained generative web gives the dark sections depth without competing
// with the copy. It is intentionally canvas based so it stays cheap to render
// and does not add another visual asset to the page.
const ambientCanvas = document.createElement("canvas");
ambientCanvas.className = "ambient-canvas";
ambientCanvas.setAttribute("aria-hidden", "true");
document.documentElement.append(ambientCanvas);
const ambientContext = ambientCanvas.getContext("2d");
const ambientPointer = { x: 0.72, y: 0.28, active: false };
const ambientColumns = 9;
const ambientRows = 7;
const ambientPoints = Array.from(
  { length: ambientColumns * ambientRows },
  (_, index) => {
    const column = index % ambientColumns;
    const row = Math.floor(index / ambientColumns);
    const jitterX = ((index * 0.754877666 + 0.17) % 1 - 0.5) * 0.32;
    const jitterY = ((index * 0.569840291 + 0.41) % 1 - 0.5) * 0.35;
    return {
      x: (column + 0.5 + jitterX) / ambientColumns,
      y: (row + 0.5 + jitterY) / ambientRows,
      radius: 0.9 + (index % 5) * 0.35,
      phase: index * 0.73,
    };
  },
);
let ambientFrame = 0;
let ambientLastDraw = 0;
let ambientWidth = 0;
let ambientHeight = 0;
let ambientScroll = 0;
const resizeAmbientCanvas = () => {
  // Keep the decorative layer below a 1600px render width. It stays sharp
  // enough for thin lines while avoiding a multi-million-pixel repaint on
  // high-density displays.
  const ratio = Math.min(
    window.devicePixelRatio || 1,
    1.5,
    1600 / Math.max(window.innerWidth, 1),
  );
  ambientWidth = window.innerWidth;
  ambientHeight = window.innerHeight;
  ambientCanvas.width = ambientWidth * ratio;
  ambientCanvas.height = ambientHeight * ratio;
  ambientCanvas.style.width = `${ambientWidth}px`;
  ambientCanvas.style.height = `${ambientHeight}px`;
  ambientContext?.setTransform(ratio, 0, 0, ratio, 0, 0);
};
/** @param {number} time */
const drawAmbientCanvas = (time) => {
  if (!ambientContext || motionPaused || document.hidden) {
    ambientFrame = 0;
    return;
  }
  // The background only needs a calm 30fps rhythm. The rest of the page can
  // still use native transitions and scroll-linked effects at full refresh.
  if (time - ambientLastDraw < 1000 / 30) {
    ambientFrame = window.requestAnimationFrame(drawAmbientCanvas);
    return;
  }
  ambientLastDraw = time;
  const ctx = ambientContext;
  const seconds = time / 1000;
  ctx.clearRect(0, 0, ambientWidth, ambientHeight);
  const pointerX = ambientPointer.active ? ambientPointer.x * ambientWidth : ambientWidth * 0.72;
  const pointerY = ambientPointer.active ? ambientPointer.y * ambientHeight : ambientHeight * 0.28;
  const glow = ctx.createRadialGradient(pointerX, pointerY, 0, pointerX, pointerY, Math.min(ambientWidth, ambientHeight) * 0.5);
  glow.addColorStop(0, "rgba(255, 121, 81, .2)");
  glow.addColorStop(0.38, "rgba(239, 177, 120, .06)");
  glow.addColorStop(1, "rgba(239, 177, 120, 0)");
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, ambientWidth, ambientHeight);
  const points = ambientPoints
    .filter((_, index) => ambientWidth >= 640 || index % 2 === 0)
    .map((point) => {
      const baseX = point.x * ambientWidth + Math.sin(seconds * 0.16 + point.phase) * 24;
      const baseY =
        point.y * ambientHeight +
        Math.cos(seconds * 0.13 + point.phase) * 18 -
        (ambientScroll - 0.5) * 34;
      const pointerInfluence = Math.max(
        0,
        1 - Math.hypot(baseX - pointerX, baseY - pointerY) / 360,
      );
      return {
        x: baseX + (pointerX - baseX) * pointerInfluence * 0.035,
        y: baseY + (pointerY - baseY) * pointerInfluence * 0.035,
        radius: point.radius,
      };
    });
  const maxLinkDistance = Math.min(290, Math.max(180, ambientWidth * 0.24));
  const links = new Set();
  ctx.lineWidth = 0.75;
  for (let index = 0; index < points.length; index += 1) {
    const point = points[index];
    const neighbors = [];
    for (let next = 0; next < points.length; next += 1) {
      if (next === index) continue;
      const candidate = points[next];
      const distance = Math.hypot(point.x - candidate.x, point.y - candidate.y);
      if (distance < maxLinkDistance) neighbors.push({ index: next, distance });
    }
    neighbors
      .sort((left, right) => left.distance - right.distance)
      .slice(0, 4)
      .forEach(({ index: neighborIndex, distance }) => {
        const linkKey = index < neighborIndex ? `${index}:${neighborIndex}` : `${neighborIndex}:${index}`;
        if (links.has(linkKey)) return;
        links.add(linkKey);
        const neighbor = points[neighborIndex];
        const midpointX = (point.x + neighbor.x) / 2;
        const midpointY = (point.y + neighbor.y) / 2;
        const pointerFocus = Math.max(
          0,
          1 - Math.hypot(midpointX - pointerX, midpointY - pointerY) / 320,
        );
        const wave =
          0.55 +
          Math.sin(seconds * 0.95 + index * 0.37 + neighborIndex * 0.19 - distance * 0.012) *
            0.45;
        const strength =
          0.055 + wave * 0.095 + pointerFocus * 0.05 - (distance / maxLinkDistance) * 0.025;
        ctx.strokeStyle = `rgba(220, 181, 164, ${Math.max(0.045, strength)})`;
        ctx.beginPath();
        ctx.moveTo(point.x, point.y);
        ctx.lineTo(neighbor.x, neighbor.y);
        ctx.stroke();
      });
  }
  points.forEach((point, index) => {
    const pulse = 0.65 + Math.sin(seconds * 0.8 + index) * 0.2;
    const pointerDistance = Math.hypot(point.x - pointerX, point.y - pointerY);
    const focus = Math.max(0, 1 - pointerDistance / 280);
    ctx.fillStyle = `rgba(255, 151, 119, ${0.24 * pulse + focus * 0.16})`;
    ctx.beginPath();
    ctx.arc(point.x, point.y, point.radius * pulse + focus * 1.6, 0, Math.PI * 2);
    ctx.fill();
  });
  ambientFrame = window.requestAnimationFrame(drawAmbientCanvas);
};
resizeAmbientCanvas();
window.addEventListener("resize", resizeAmbientCanvas, { passive: true });

/** @type {HTMLDialogElement | null} */
const lightbox = document.querySelector("[data-lightbox-dialog]");
/** @type {HTMLImageElement | null} */
const lightboxImage = lightbox?.querySelector("[data-lightbox-image]") ?? null;
/** @type {HTMLElement | null} */
const lightboxCaption =
  lightbox?.querySelector("[data-lightbox-caption]") ?? null;
/** @type {NodeListOf<HTMLAnchorElement>} */
const lightboxTriggers = document.querySelectorAll("[data-lightbox]");
/** @type {HTMLAnchorElement | null} */
let activeLightboxTrigger = null;

const closeLightbox = () => lightbox?.close();

lightboxTriggers.forEach((trigger) => {
  trigger.addEventListener("click", (event) => {
    if (!lightbox || !lightboxImage || !lightboxCaption) return;

    event.preventDefault();
    const preview = trigger.querySelector("img");
    lightboxImage.src = trigger.href;
    lightboxImage.alt = preview?.alt ?? "";
    lightboxCaption.textContent =
      trigger.dataset.lightboxCaption ?? preview?.alt ?? "";
    activeLightboxTrigger = trigger;
    lightbox.showModal();
    document.body.classList.add("lightbox-open");
  });
});

lightbox
  ?.querySelector("[data-lightbox-close]")
  ?.addEventListener("click", closeLightbox);
lightbox?.addEventListener("click", (event) => {
  if (event.target === lightbox) closeLightbox();
});
lightbox?.addEventListener("close", () => {
  document.body.classList.remove("lightbox-open");
  lightboxImage?.removeAttribute("src");
  activeLightboxTrigger?.focus();
  activeLightboxTrigger = null;
});

// Decorative motion is built into the page, so the landing stays self-contained
// and does not need a query flag or a settings control.
/** @type {HTMLElement | null} */
const stage = document.querySelector("[data-pointer-stage]");
/** @type {HTMLElement | null} */
const tiltCard = stage?.querySelector("[data-tilt]") ?? null;
const finePointer = window.matchMedia("(hover: hover) and (pointer: fine)");
const motionPaused = false;
let pointerFrame = 0;
/** @type {Set<Animation>} */
const entranceAnimations = new Set();
const resetTilt = () => {
  window.cancelAnimationFrame(pointerFrame);
  pointerFrame = 0;
  tiltCard?.style.removeProperty("--tilt-x");
  tiltCard?.style.removeProperty("--tilt-y");
};
const updateMotion = () => {
  const paused = motionPaused;
  document.documentElement.dataset.motionState = paused ? "off" : "on";
  if (paused) entranceAnimations.forEach((animation) => animation.finish());
  stage?.toggleAttribute("data-motion-paused", paused);
  if (paused) resetTilt();
  if (paused || document.hidden) {
    window.cancelAnimationFrame(ambientFrame);
    ambientFrame = 0;
    ambientContext?.clearRect(0, 0, ambientWidth, ambientHeight);
  } else if (!ambientFrame) {
    ambientFrame = window.requestAnimationFrame(drawAmbientCanvas);
  }
};
document.addEventListener("visibilitychange", updateMotion);
stage?.addEventListener("pointermove", (event) => {
  if (motionPaused || !finePointer.matches) return;
  const box = stage.getBoundingClientRect();
  const x = (event.clientX - box.left) / box.width;
  const y = (event.clientY - box.top) / box.height;
  window.cancelAnimationFrame(pointerFrame);
  pointerFrame = window.requestAnimationFrame(() => {
    tiltCard?.style.setProperty("--tilt-x", `${(y - 0.5) * -5}deg`);
    tiltCard?.style.setProperty("--tilt-y", `${(x - 0.5) * 6}deg`);
    stage.style.setProperty("--pointer-x", `${x * 100}%`);
    stage.style.setProperty("--pointer-y", `${y * 100}%`);
  });
});
stage?.addEventListener("pointerleave", resetTilt);
updateMotion();

const updateScrollProgress = () => {
  const scrollable = document.documentElement.scrollHeight - window.innerHeight;
  const progress = scrollable > 0 ? window.scrollY / scrollable : 0;
  ambientScroll = progress;
  document.documentElement.style.setProperty("--scroll-progress", `${progress}`);
};
let scrollFrame = 0;
window.addEventListener(
  "scroll",
  () => {
    if (scrollFrame) return;
    scrollFrame = window.requestAnimationFrame(() => {
      updateScrollProgress();
      scrollFrame = 0;
    });
  },
  { passive: true },
);
updateScrollProgress();

if (finePointer.matches) {
  window.addEventListener(
    "pointermove",
    (event) => {
      ambientPointer.x = event.clientX / window.innerWidth;
      ambientPointer.y = event.clientY / window.innerHeight;
      ambientPointer.active = true;
    },
    { passive: true },
  );
}
ambientFrame = window.requestAnimationFrame(drawAmbientCanvas);

document.querySelectorAll(".button, .nav-download").forEach((control) => {
  if (!(control instanceof HTMLElement)) return;
  control.addEventListener("pointermove", (event) => {
    if (motionPaused || !finePointer.matches) return;
    const bounds = control.getBoundingClientRect();
    const x = (event.clientX - bounds.left) / bounds.width - 0.5;
    const y = (event.clientY - bounds.top) / bounds.height - 0.5;
    control.style.setProperty("--mag-x", `${x * 7}px`);
    control.style.setProperty("--mag-y", `${y * 5}px`);
  });
  control.addEventListener("pointerleave", () => {
    control.style.removeProperty("--mag-x");
    control.style.removeProperty("--mag-y");
  });
  control.addEventListener("click", () => {
    control.classList.remove("is-clicked");
    void control.offsetWidth;
    control.classList.add("is-clicked");
  });
});

// Animate sections once on entry; native content visibility never depends on JS.
if ("IntersectionObserver" in window) {
  const entranceObserver = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        entranceObserver.unobserve(entry.target);
        if (motionPaused) continue;
        const animation = entry.target.animate(
          [
            { opacity: 0.15, transform: "translateY(22px)" },
            { opacity: 1, transform: "translateY(0)" },
          ],
          { duration: 650, easing: "cubic-bezier(.22,1,.36,1)" },
        );
        entranceAnimations.add(animation);
        animation.addEventListener(
          "finish",
          () => entranceAnimations.delete(animation),
          { once: true },
        );
      }
    },
    { threshold: 0.12 },
  );
  document
    .querySelectorAll(
      ".section-heading, .features, .proof-copy, .product-proof figure, .workflow-grid, .control-copy, .control figure, .faq, .download-panel",
    )
    .forEach((element) => entranceObserver.observe(element));
}
