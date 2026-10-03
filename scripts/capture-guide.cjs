// Real UI fragments for the first-run guide; no personal data or microphone capture.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const { fixture } = require('./capture-gallery.cjs');
const fs = require('node:fs');
const path = require('node:path');
const origin = process.env.GALLERY_ORIGIN || 'http://127.0.0.1:1428';
const out = path.resolve(__dirname, '../public/landing/guide');
fs.mkdirSync(out, { recursive: true });

(async () => {
  const browser = await chromium.launch({ headless: true, ...(process.env.CHROMIUM_EXECUTABLE_PATH ? { executablePath: process.env.CHROMIUM_EXECUTABLE_PATH } : {}) });
  try {
    for (const lang of ['en', 'ru']) {
      const ru = lang === 'ru';
      const page = await browser.newPage({ viewport: { width: 900, height: 1100 }, deviceScaleFactor: 2, reducedMotion: 'reduce' });
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.route(origin + '/', async route => {
        const response = await route.fetch();
        const html = await response.text();
        await route.fulfill({ response, body: html.replace(/<script type="module" src="\/src\/main\.tsx[^"]*"><\/script>/, '<script type="module">' + fixture(lang) + '</script>') });
      });
      await page.goto(origin + '/');
      const button = name => page.getByRole('button', { name, exact: true });
      await button(ru ? 'Настройки' : 'Settings').waitFor();
      await page.evaluate(() => document.fonts.ready);
      const shot = async (name, locator) => locator.screenshot({ path: path.join(out, `${name}-${lang}.png`), animations: 'disabled' });

      await page.evaluate(async () => {
        const s = window.galleryDemo.status();
        const model = { ...s.models[0], id: 'roen-tiny', name: 'RuEn · Tiny', description: 'Smallest general multilingual Whisper build. Fastest option for short Russian/English dictation.', format: 'CTranslate2 · FP16 weights', size_mb: 78, repo_id: 'Systran/faster-whisper-tiny', source_url: 'https://huggingface.co/Systran/faster-whisper-tiny', tier: 'light', downloaded: false, loaded: false };
        await window.galleryDemo.set({ engine_status: 'not_installed', worker_alive: false, stt_ready: false, device: null, compute_type: null, current_model_id: null, last_text: null, last_copied: false, models: [model] });
      });
      await button(ru ? 'Установить / обновить движок' : 'Install / update engine').waitFor();
      await shot('engine', page.locator('.engine-panel'));

      await page.evaluate(() => window.galleryDemo.set({ engine_status: 'stopped', worker_alive: true }));
      await page.setViewportSize({ width: 650, height: 1000 });
      await button(ru ? 'Модели' : 'Manage models').click();
      await page.locator('.model-card').waitFor();
      await shot('model', page.locator('.model-card').first());
      await page.keyboard.press('Escape');

      await page.setViewportSize({ width: 760, height: 1100 });
      await button(ru ? 'Настройки' : 'Settings').click();
      await button(ru ? 'Начать тест' : 'Start test').click();
      await page.waitForFunction(() => Number(document.querySelector('[role="meter"]').getAttribute('aria-valuenow')) > 0);
      const device = page.locator('.microphone-device');
      await device.scrollIntoViewIfNeeded();
      const deviceBox = await device.boundingBox();
      const testBox = await page.locator('.microphone-test').boundingBox();
      if (!deviceBox || !testBox) throw new Error('Microphone controls not visible');
      await page.screenshot({ path: path.join(out, `microphone-${lang}.png`), animations: 'disabled', clip: { x: deviceBox.x, y: deviceBox.y, width: deviceBox.width, height: testBox.y + testBox.height - deviceBox.y } });
      await button(ru ? 'Закрыть настройки' : 'Close settings').click();

      await page.setViewportSize({ width: 1120, height: 660 });
      await page.evaluate(async () => {
        const s = window.galleryDemo.status();
        await window.galleryDemo.set({ engine_status: 'ready', stt_ready: true, current_model_id: s.models[0].id, models: s.models.map(m => ({ ...m, downloaded: true, loaded: true })), last_text: document.documentElement.lang === 'ru' ? 'Отправь команде план на завтра. Проверь Docker и обнови README.' : 'Send the team tomorrow’s plan. Check Docker and update the README.', last_copied: true });
      });
      await page.locator('.card-transcript .transcript:not(.empty)').waitFor();
      await shot('result', page.locator('.card-transcript'));
      if (errors.length) throw new Error(errors.join('\n'));
      console.log(`Captured four ${lang} guide screenshots.`);
      await page.close();
    }
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
