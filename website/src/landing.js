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
stage?.addEventListener("pointermove", (event) => {
  const box = stage.getBoundingClientRect();
  stage.style.setProperty("--pointer-x", `${event.clientX - box.left}px`);
  stage.style.setProperty("--pointer-y", `${event.clientY - box.top}px`);
});

/** @type {NodeListOf<HTMLElement>} */
const tiltElements = document.querySelectorAll("[data-tilt]");
tiltElements.forEach((element) => {
  element.addEventListener("pointermove", (event) => {
    const box = element.getBoundingClientRect();
    const x = (event.clientX - box.left) / box.width - 0.5;
    const y = (event.clientY - box.top) / box.height - 0.5;
    element.style.setProperty("--tilt-x", `${y * -4}deg`);
    element.style.setProperty("--tilt-y", `${x * 5}deg`);
  });
  element.addEventListener("pointerleave", () => {
    element.style.removeProperty("--tilt-x");
    element.style.removeProperty("--tilt-y");
  });
});
