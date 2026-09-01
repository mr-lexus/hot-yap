// @ts-check

document.documentElement.classList.add("has-js");

/** @type {HTMLElement | null} */
const year = document.querySelector("[data-current-year]");
if (year) year.textContent = String(new Date().getFullYear());

/** @type {NodeListOf<HTMLElement>} */
const reveal = document.querySelectorAll("[data-reveal]");
if ("IntersectionObserver" in window) {
  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        entry.target.classList.add("is-visible");
        observer.unobserve(entry.target);
      }
    },
    { rootMargin: "0px 0px -8%", threshold: 0.12 },
  );
  reveal.forEach((element) => observer.observe(element));
} else {
  reveal.forEach((element) => element.classList.add("is-visible"));
}

/** @type {HTMLElement | null} */
const stage = document.querySelector("[data-pointer-stage]");
let pointerFrame = 0;
stage?.addEventListener("pointermove", (event) => {
  const box = stage.getBoundingClientRect();
  const x = event.clientX - box.left;
  const y = event.clientY - box.top;
  if (pointerFrame) return;
  pointerFrame = window.requestAnimationFrame(() => {
    stage.style.setProperty("--pointer-x", `${x}px`);
    stage.style.setProperty("--pointer-y", `${y}px`);
    pointerFrame = 0;
  });
});

/** @type {NodeListOf<HTMLElement>} */
const tiltElements = document.querySelectorAll("[data-tilt]");
tiltElements.forEach((element) => {
  let tiltFrame = 0;
  element.addEventListener("pointermove", (event) => {
    const box = element.getBoundingClientRect();
    const x = (event.clientX - box.left) / box.width - 0.5;
    const y = (event.clientY - box.top) / box.height - 0.5;
    if (tiltFrame) return;
    tiltFrame = window.requestAnimationFrame(() => {
      element.style.setProperty("--tilt-x", `${y * -4}deg`);
      element.style.setProperty("--tilt-y", `${x * 5}deg`);
      tiltFrame = 0;
    });
  });
  element.addEventListener("pointerleave", () => {
    if (tiltFrame) window.cancelAnimationFrame(tiltFrame);
    tiltFrame = 0;
    element.style.removeProperty("--tilt-x");
    element.style.removeProperty("--tilt-y");
  });
});

/** @type {HTMLDialogElement | null} */
const lightbox = document.querySelector("[data-lightbox-dialog]");
/** @type {HTMLImageElement | null} */
const lightboxImage = lightbox?.querySelector("[data-lightbox-image]") ?? null;
/** @type {HTMLElement | null} */
const lightboxCaption = lightbox?.querySelector("[data-lightbox-caption]") ?? null;
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

lightbox?.querySelector("[data-lightbox-close]")?.addEventListener("click", closeLightbox);
lightbox?.addEventListener("click", (event) => {
  if (event.target === lightbox) closeLightbox();
});
lightbox?.addEventListener("close", () => {
  document.body.classList.remove("lightbox-open");
  lightboxImage?.removeAttribute("src");
  activeLightboxTrigger?.focus();
  activeLightboxTrigger = null;
});
