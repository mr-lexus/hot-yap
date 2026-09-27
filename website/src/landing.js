// @ts-check

/** @type {HTMLElement | null} */
const year = document.querySelector("[data-current-year]");
if (year) year.textContent = String(new Date().getFullYear());

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

// Keep decorative motion optional and respect the system preference.
/** @type {HTMLElement | null} */
const stage = document.querySelector("[data-pointer-stage]");
/** @type {HTMLElement | null} */
const tiltCard = stage?.querySelector("[data-tilt]") ?? null;
/** @type {HTMLButtonElement | null} */
const motionButton = document.querySelector("[data-motion-toggle]");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
const finePointer = window.matchMedia("(hover: hover) and (pointer: fine)");
const russian = document.documentElement.lang === "ru";
/** @type {boolean | null} */
let motionPreference = null;
try {
  const savedMotion = sessionStorage.getItem("hotyap:motion");
  if (savedMotion === "on" || savedMotion === "off")
    motionPreference = savedMotion === "off";
} catch {
  /* Motion still works when storage is unavailable. */
}
let motionPaused = motionPreference ?? reducedMotion.matches;
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
  if (motionButton) {
    motionButton.hidden = false;
    motionButton.setAttribute("aria-pressed", String(paused));
    motionButton.textContent = russian
      ? paused
        ? "Включить анимацию"
        : "Остановить анимацию"
      : paused
        ? "Resume animation"
        : "Pause animation";
  }
  if (paused) resetTilt();
};
motionButton?.addEventListener("click", () => {
  motionPaused = !motionPaused;
  motionPreference = motionPaused;
  try {
    sessionStorage.setItem("hotyap:motion", motionPaused ? "off" : "on");
  } catch {
    /* Optional preference. */
  }
  updateMotion();
});
reducedMotion.addEventListener("change", () => {
  motionPaused = motionPreference ?? reducedMotion.matches;
  updateMotion();
});
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
