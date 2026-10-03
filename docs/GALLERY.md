# Product screenshots

The gallery contains six scenarios in English and Russian: live dictation, personal dictionary, project terminology review, microphone test, dictation/delivery settings, and the light theme. Captures render the real desktop React components with deterministic demonstration data via Tauri's official IPC mocks. No microphone recording, transcription benchmark, personal file, or API key is used.

Images: `public/landing/gallery/*.png` (1680 × 1200). The site offers explicit tabs, arrow buttons, keyboard navigation, and a click-to-expand dialog. There is no automatic slide rotation. Descriptions and image alt text are present in the static HTML in both languages.

## Regenerate

1. Install Playwright in your development tools environment and install a Chromium browser.
2. Start `pnpm exec vite --host 127.0.0.1 --port 1428 --strictPort`.
3. Run `node scripts/capture-gallery.cjs`.

If Playwright is installed outside this repository, set `PLAYWRIGHT_MODULE` to the absolute path of its module. Optionally set `CHROMIUM_EXECUTABLE_PATH` to an installed Chrome/Chromium executable; otherwise Playwright uses its bundled Chromium. `GALLERY_ORIGIN` can override the default Vite address.

After regeneration, inspect the images, run `pnpm site:build`, and preview under `/hot-yap/` and `/hot-yap/ru/`. Check all six tabs, arrow-key/Home/End selection, lightbox close/focus return, image loading, and small-screen overflow. Demonstration content should stay clearly labeled on the site and in the README.

## First-run guide

The `#how` section explains engine installation, model download/loading, microphone testing, and the first dictation with clipboard paste. Its eight English/Russian screenshots are real UI fragments captured separately for legibility, using the same isolated demonstration fixtures.

With the same Vite server and environment variables, run `node scripts/capture-guide.cjs` to regenerate `public/landing/guide/*.png`. No real microphone, user settings, or installed app is accessed. Keep the text instructions available without JavaScript and use the existing lightbox for larger previews. Verify both locales at desktop and mobile widths, including image loading, Escape/focus return, and the optional settings disclosure.
