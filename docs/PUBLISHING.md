# Публикация HotYap: лендинг, пакеты и alpha-релизы

Этот документ описывает подготовленную схему публикации HotYap: где находится лендинг, как он собирается и индексируется, как GitHub Actions создаёт установщики, как выпустить alpha-релиз и какие ограничения пока остаются.

## 1. Текущий статус

Канал выпуска: **`0.1.0-alpha.1`**.

Это alpha, а не beta, по следующим причинам:

- публичных релизов и кроссплатформенной статистики использования ещё нет;
- Linux — основная проверенная платформа;
- Windows и macOS собираются автоматически, но требуют расширенного тестирования микрофона, хоткеев, оверлея и системного хранилища ключей;
- установщики пока не подписаны сертификатами Windows и Apple;
- API и формат локальных данных ещё могут меняться.

Версия приложения внутри `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` и `backend/worker.py` остаётся `0.1.0`. Суффикс канала находится в Git-теге `v0.1.0-alpha.1` и имени GitHub prerelease.

## 2. Публичные ссылки

- Репозиторий: <https://github.com/mr-lexus/hot-yap>
- Английский лендинг: <https://mr-lexus.github.io/hot-yap/>
- Русский лендинг: <https://mr-lexus.github.io/hot-yap/ru/>
- Релизы: <https://github.com/mr-lexus/hot-yap/releases>
- Задачи и ошибки: <https://github.com/mr-lexus/hot-yap/issues>
- GitHub Actions: <https://github.com/mr-lexus/hot-yap/actions>
- Sitemap: <https://mr-lexus.github.io/hot-yap/sitemap.xml>
- Robots: <https://mr-lexus.github.io/hot-yap/robots.txt>

Ссылки начнут работать после первого коммита, push в `main`, включения Pages и успешного workflow.

## 3. Структура лендинга

Лендинг отделён от Tauri UI, чтобы браузерная сборка не импортировала нативные Tauri API и не меняла `dist/`, который нужен десктопному приложению.

| Путь | Назначение |
|---|---|
| `website/index.html` | Английская SEO-страница |
| `website/ru/index.html` | Русская SEO-страница |
| `website/src/landing.css` | Адаптивный визуальный стиль лендинга |
| `website/src/landing.js` | Reveal-анимации и лёгкая pointer-интерактивность; работает и в обычном Live Server |
| `vite.pages.config.ts` | Независимая Vite multipage-сборка |
| `tsconfig.pages.json` | Отдельная TypeScript-проверка сайта |
| `dist-pages/` | Сгенерированный сайт; в Git не добавляется |
| `website/assets/` | Физические изображения лендинга для Vite и обычного Live Server |
| `public/landing/` | Публичные SEO-копии OG-карточки и скриншотов с постоянными URL |

Обычная Tauri-сборка по-прежнему использует корневые `index.html`, `vite.config.ts`, `src/` и выходной каталог `dist/`.

## 4. Локальный запуск лендинга

Требования: Node.js 22.12+ и pnpm 11.18.

```bash
pnpm install --frozen-lockfile
pnpm site:dev
```

Локальная версия открывается на адресе, который напечатает Vite, обычно <http://localhost:5173/>. Русская страница находится по пути `/ru/`.

Production-сборка и просмотр:

```bash
pnpm site:build
pnpm site:preview
```

Результат находится в `dist-pages/`.

Для точного воспроизведения GitHub Pages base path:

```bash
GITHUB_ACTIONS=true GITHUB_REPOSITORY=mr-lexus/hot-yap pnpm site:build
GITHUB_ACTIONS=true GITHUB_REPOSITORY=mr-lexus/hot-yap pnpm site:preview
```

Vite автоматически ставит префикс `/hot-yap/` перед CSS, JavaScript и публичными изображениями. При форке имя base path берётся из `GITHUB_REPOSITORY`. Его можно явно заменить переменной `PAGES_BASE`.

## 5. SEO

Обе языковые страницы содержат текст непосредственно в HTML, а не только в клиентском React, поэтому поисковый робот видит контент без выполнения JavaScript.

Реализовано:

- отдельные `title` и `description` для EN и RU;
- canonical URL;
- `hreflang="en"`, `hreflang="ru"` и `x-default`;
- Open Graph и Twitter Card;
- JSON-LD `SoftwareApplication`;
- `robots.txt`;
- `sitemap.xml` с обеими локалями;
- семантические заголовки, alt-тексты и доступная skip-link;
- адаптивность и `prefers-reduced-motion`;
- статическая OG-картинка 1200×630.

После публикации рекомендуется добавить сайт в Google Search Console и Яндекс Вебмастер, затем отправить URL sitemap:

```text
https://mr-lexus.github.io/hot-yap/sitemap.xml
```

Если появится собственный домен, нужно обновить canonical, Open Graph URL, sitemap, robots, manifest и добавить `CNAME`.

## 6. GitHub Pages

Workflow: `.github/workflows/pages.yml`.

Он запускается вручную или при изменениях лендинга в `main`, выполняет `pnpm site:build`, загружает `dist-pages/` как Pages artifact и публикует его через официальный `actions/deploy-pages`.

Первичная настройка репозитория:

1. Создать первый коммит и отправить `main` в `git@github.com:mr-lexus/hot-yap.git`.
2. Открыть `Settings → Pages`.
3. В разделе `Build and deployment → Source` выбрать `GitHub Actions`.
4. Запустить `Deploy landing to Pages` вручную или отправить изменение в `main`.
5. Проверить environment `github-pages` и выданный workflow URL.

Workflow использует минимальные разрешения `contents: read`, `pages: write`, `id-token: write` и concurrency-группу, чтобы старый deploy отменялся новым.

## 7. CI-проверки

Workflow: `.github/workflows/ci.yml`.

При push и pull request он выполняет:

- production-сборку desktop frontend;
- production-сборку двуязычного лендинга;
- Rust unit tests с `Cargo.lock`;
- синтаксическую проверку Python-файлов.

Локальный эквивалент:

```bash
pnpm build
pnpm site:build
cargo test --locked --manifest-path src-tauri/Cargo.toml
python3 -m compileall -q backend
```

## 8. Устройство standalone worker

Раньше production-бинарник искал `backend/.venv/bin/python` и `backend/worker.py` внутри исходного checkout. Такой путь работал в `tauri dev`, но ломал установленный пакет.

Теперь используются два режима:

- Development: Rust запускает Python из `backend/.venv/` и исходный `backend/worker.py`.
- Release: Actions собирает `hotyap-worker` через PyInstaller и Tauri кладёт его рядом с основным executable как sidecar.

Код выбора находится в `src-tauri/src/worker.rs`. Release-only настройки находятся в `src-tauri/tauri.release.conf.json`, поэтому обычный `pnpm tauri dev` не требует наличия sidecar-файла.

Точные верхнеуровневые зависимости release worker зафиксированы в `backend/requirements-release.txt`. Workflow дополнительно запускает JSONL smoke test готового worker до упаковки приложения.

### CUDA runtime в Windows-бандле

Windows-версия worker поддерживает GPU-инференс Whisper через cuBLAS. `ctranslate2.dll` загружает `cublas64_12.dll` в рантайме через `LoadLibrary` (без статического импорта), поэтому PyInstaller не находит его автоматически. Чтобы конечному пользователю не требовался установленный CUDA Toolkit:

- Windows-шаг workflow ставит wheel `nvidia-cublas-cu12==12.4.5.8` (официальный redistributable-компонент NVIDIA) и добавляет в бандл ровно два файла из него: `cublas64_12.dll` и `cublasLt64_12.dll` (статическая зависимость cuBLAS).
- PyInstaller onefile кладёт их в корень распаковки и добавляет этот каталог в DLL search path через `SetDllDirectoryW`, так что `LoadLibrary("cublas64_12.dll")` находит их без изменения `PATH`.
- `cudart64_12.dll` и cuDNN в бандл не попадают: ни `ctranslate2.dll`, ни cuBLAS 12 на них не ссылаются (драйвер используется напрямую через `nvcuda.dll`).
- Windows smoke test гоняет команду `verify_cuda_runtime`, которая загружает обе DLL через `ctypes.WinDLL` и падает на CI при отсутствии хотя бы одной. Загрузка DLL не требует GPU, поэтому проверка работает на runner без видеокарты.
- Если runtime всё же отсутствует (например, dev-сборка или повреждённый бандл), приложение предлагает скачать его: команда `download_cuda_runtime` забирает официальный wheel `nvidia-cublas-cu12` с PyPI, извлекает только две DLL в `<models>/cuda-runtime/` и добавляет каталог в DLL search path через `os.add_dll_directory`. UI показывает баннер с кнопкой «Скачать CUDA runtime» и прогрессом.
- Лицензия NVIDIA cuBLAS: `backend/licenses/NVIDIA-cuBLAS-License.txt` (EULA из wheel 12.4.5.8). Дистрибуция разрешена как встроенный в приложение объектный код при соблюдении условий EULA.

Ограничение: CPU-only пользователи Windows также получают ~570 MB cuBLAS DLL внутри worker (универсальный sidecar). Разделение CPU/CUDA worker'ов запланировано как отдельный шаг.

Модели Whisper не включены в sidecar и установщик. Они загружаются по выбору пользователя в app data. Это сохраняет разумный размер релиза и позволяет выбирать модели от tiny до large-v3.

Для ручной отладки можно переопределить запуск:

```bash
VOXSHIFT_PYTHON=/path/to/python pnpm tauri dev
VOXSHIFT_WORKER=/path/to/hotyap-worker pnpm tauri dev
```

## 9. Release workflow и пакеты

Workflow: `.github/workflows/release.yml`.

Матрица выпуска:

| ОС | Runner | Target | Файлы в релизе |
|---|---|---|---|
| Linux x86_64 | `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | DEB, RPM, AppImage |
| Windows x86_64 | `windows-2022` | `x86_64-pc-windows-msvc` | MSI, NSIS EXE |
| macOS Intel | `macos-15-intel` | `x86_64-apple-darwin` | DMG |
| macOS Apple Silicon | `macos-14` | `aarch64-apple-darwin` | DMG |

Каждая задача выполняет один и тот же порядок:

1. Устанавливает Node, pnpm, Python 3.12 и Rust target.
2. Устанавливает системные Tauri-зависимости на Linux.
3. Собирает нативный PyInstaller worker текущей архитектуры.
4. Проверяет worker через JSON Lines: на Linux/macOS командами `status` и `shutdown`, на Windows — `verify_cuda_runtime` (загрузка CUDA runtime DLL) и `shutdown`.
5. Запускает `tauri-apps/tauri-action` с release-конфигурацией.
6. Добавляет пакеты в GitHub prerelease.

## 10. Как выпустить alpha

Перед тегом синхронизировать базовую версию в этих файлах:

```text
package.json
src-tauri/Cargo.toml
src-tauri/tauri.conf.json
backend/worker.py
```

Первый релиз:

```bash
git tag v0.1.0-alpha.1
git push origin v0.1.0-alpha.1
```

Тег должен соответствовать шаблону `v*-alpha.*`. Workflow автоматически создаст публичный prerelease и добавит пакеты по мере завершения четырёх matrix jobs.

Альтернативный путь: открыть `Actions → Build alpha release → Run workflow`, указать `v0.1.0-alpha.1` и запустить вручную.

Перед анонсом проверить:

- все четыре matrix job зелёные;
- в релизе есть DEB, RPM, AppImage, MSI, NSIS EXE и два DMG;
- release помечен как `Pre-release`;
- checksum и размеры файлов выглядят разумно;
- Linux-пакет запускается на чистой машине;
- первый запуск позволяет скачать, загрузить модель и сделать короткую диктовку;
- Windows и macOS корректно запрашивают доступ к микрофону.

## 11. Подпись и доверие ОС

Текущие alpha-пакеты неподписанные. Это не препятствует сборке, но вызывает предупреждения Windows SmartScreen и macOS Gatekeeper.

Для beta понадобятся:

- Windows code-signing certificate и timestamp server;
- Apple Developer ID Application certificate;
- Apple notarization и stapling;
- подпись основного приложения и PyInstaller sidecar одной цепочкой доверия;
- проверка установки на чистых Windows 10/11 и поддерживаемых macOS.

`src-tauri/Info.plist` уже содержит `NSMicrophoneUsageDescription` для macOS.

## 12. Скриншоты и графика

| Файл | Содержание |
|---|---|
| `website/assets/screenshots/hotyap-app.png` | Реальное окно desktop-приложения |
| `website/assets/screenshots/hotyap-landing.png` | Desktop hero лендинга 1440×1000 |
| `website/assets/screenshots/hotyap-landing-mobile.png` | Русская mobile-версия 390×844 |
| `website/assets/og-cover.png` | Social preview 1200×630 |
| `website/assets/favicon-dark.png` / `favicon-light.png` | Web-иконка (тема светлая/тёмная) |
| `public/landing/` | Копии для стабильных Open Graph URL на GitHub Pages |

Скриншот приложения снят из реально запущенного Tauri dev build. Лендинг снят headless Chrome из production-сборки с base path `/hot-yap/`.

## 13. Известные ограничения

- В репозитории пока не выбрана лицензия. До публичного позиционирования как open source нужно добавить согласованный `LICENSE`; автоматически выбирать лицензию нельзя.
- Установщики не подписаны и не нотаризованы.
- Release requirements фиксируют верхнеуровневые Python-пакеты, но пока не являются полным hash-locked набором всех транзитивных wheels.
- AppImage всё равно зависит от совместимости glibc и доступности системного WebKitGTK на конкретном дистрибутиве.
- Глобальный хоткей Linux ориентирован на X11. На Wayland остаётся кнопка в интерфейсе.
- Windows и macOS workflow готовы, но первый реальный прогон Actions и smoke test установщиков возможен только после push исходников.
- Идентификатор `com.voxshift.app` сохранён ради существующих моделей, хотя Tauri предупреждает о суффиксе `.app` на macOS.

## 14. Что не выполняется автоматически

Workflow не создаёт первый Git-коммит, не включает Pages в настройках репозитория, не покупает сертификаты, не выбирает лицензию и не публикует тег без явного действия владельца.

Это намеренные ограничения: Actions начинают работать только после того, как исходники окажутся на GitHub и владелец подтвердит канал публикации.
