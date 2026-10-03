// Capture the real React interface with deterministic demonstration data.
// Start Vite on port 1428. Install Playwright in a development tools directory,
// then set PLAYWRIGHT_MODULE to its module path if it is not on Node's path.
// CHROMIUM_EXECUTABLE_PATH optionally selects an installed Chromium browser.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const fs = require('node:fs');
const path = require('node:path');
const origin = process.env.GALLERY_ORIGIN || 'http://127.0.0.1:1428';
const out = path.resolve(__dirname, '../public/landing/gallery');
fs.mkdirSync(out, { recursive: true });

function fixture(lang) {
  return `
    const { mockIPC, mockWindows } = await import('/node_modules/@tauri-apps/api/mocks.js');
    const { DEFAULT_STATUS } = await import('/src/status.ts');
    localStorage.setItem('hotyap-language', ${JSON.stringify(lang)});
    localStorage.setItem('hotyap-theme', 'dark');
    localStorage.setItem('hotyap-accent', 'amber');
    mockWindows('main');
    const ru=${lang === 'ru'};
    const model={id:'ruen-large-v3-turbo',name:'RuEn Large v3 Turbo',description:'Multilingual Whisper · Russian + English',family:'RuEn',format:'int8_float16',size_mb:1620,repo_id:'deepdml/faster-whisper-large-v3-turbo-ct2',backend:'ctranslate2',source_url:'https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2',tags:['ru','en'],downloaded:true,loaded:true,tier:'heavy'};
    let status={...DEFAULT_STATUS,model_status:'downloaded',engine_status:'ready',device:'cuda',compute_type:'int8_float16',hotkey_registered:true,stt_ready:true,current_model_id:model.id,models:[model],stt_model:model.name,mic_name:'USB Microphone',last_text:ru?'Добавь авторизацию через Supabase. Вынеси логику в useAuth, сохрани refreshToken и обработай ошибки в TypeScript.':'Add authentication with Supabase. Move the logic into useAuth, persist the refreshToken, and handle errors in TypeScript.',last_copied:true};
    status.provider_settings={...status.provider_settings,live_transcription:true,dictation_preview:true,auto_paste:true,system_audio:'duck',history_enabled:false,providers:{}};
    let dictionary={revision:0,active_project:'atlas',learning:'suggest',projects:[{id:'atlas',name:'Atlas',path:ru?'C:/Projects/atlas':'C:/Projects/atlas'},{id:'hotyap',name:'HotYap',path:'C:/Projects/hotyap'}],entries:[
      {id:'1',heard:ru?'супабейс':'super base',written:'Supabase',project_id:null,origin:'manual',enabled:true},
      {id:'2',heard:ru?'тайп скрипт':'type script',written:'TypeScript',project_id:null,origin:'learned',enabled:true},
      {id:'3',heard:ru?'некст джи эс':'next jay ess',written:'Next.js',project_id:null,origin:'manual',enabled:true},
      {id:'4',heard:ru?'версель':'ver sell',written:'Vercel',project_id:null,origin:'manual',enabled:true},
      {id:'5',heard:ru?'юз авторизации':'use auth',written:'useAuth',project_id:'atlas',origin:'project',enabled:true},
      {id:'6',heard:ru?'рефреш токен':'refresh token',written:'refreshToken',project_id:'atlas',origin:'project',enabled:true}
    ],suggestions:[{id:'7',heard:ru?'докер компоуз':'docker compose',written:'Docker Compose',project_id:null,origin:'learned',enabled:true}]};
    let test=false;
    mockIPC((cmd,args)=>{
      if(cmd==='get_status')return structuredClone(status);
      if(cmd==='list_models')return status.models;
      if(cmd==='get_dictionary')return structuredClone(dictionary);
      if(cmd==='save_dictionary'){dictionary={...args.dictionary,revision:dictionary.revision+1};return structuredClone(dictionary)}
      if(cmd==='get_provider_settings')return structuredClone(status.provider_settings);
      if(cmd==='save_provider_settings'){status.provider_settings=args.settings;return structuredClone(args.settings)}
      if(cmd==='paste_support')return {platform:'windows',available:true,reason:'windows'};
      if(cmd==='scan_project')return {terms:['useAuth','refreshToken','AtlasClient','WorkspaceRole','Supabase','Next.js','TeamSettings','ProjectContext'],files:128,truncated:false};
      if(cmd==='list_microphones')return [{id:'usb',name:'USB Microphone',is_default:true},{id:'array',name:'Microphone Array',is_default:false}];
      if(cmd==='start_microphone_test'){test=true;return null}
      if(cmd==='stop_microphone_test'){test=false;return null}
      if(cmd==='microphone_test_status')return {active:test,level:0.105,remaining:12,mic_name:'USB Microphone',error:null};
      if(cmd==='check_cuda_runtime')return status.cuda_runtime;
      if(cmd==='plugin:window|theme')return 'dark';
      return null;
    },{shouldMockEvents:true});
    window.galleryDemo={set:async patch=>{status={...status,...patch};const {emit}=await import('/node_modules/@tauri-apps/api/event.js');await emit('vox:status',status)},theme:async(theme,accent)=>{const {applyAppearance}=await import('/src/appearance.ts');applyAppearance(theme,accent)},status:()=>status};
    await import('/src/main.tsx');
  `;
}
async function captureGallery(){
  const browser=await chromium.launch({headless:true,...(process.env.CHROMIUM_EXECUTABLE_PATH?{executablePath:process.env.CHROMIUM_EXECUTABLE_PATH}:{})});
  try {
    for(const lang of ['en','ru']) {
      const page=await browser.newPage({viewport:{width:1120,height:800},deviceScaleFactor:1.5,locale:lang==='ru'?'ru-RU':'en-US',reducedMotion:'reduce'});
      const errors=[];page.on('pageerror',e=>errors.push(e.message));
      await page.route(origin+'/',async route=>{const response=await route.fetch();const html=await response.text();await route.fulfill({response,body:html.replace(/<script type="module" src="\/src\/main\.tsx[^"]*"><\/script>/,'<script type="module">'+fixture(lang)+'</script>')});});
      await page.goto(origin+'/');
      const ru=lang==='ru';
      const button=(name)=>page.getByRole('button',{name,exact:true});
      await button(ru?'Словарь':'Dictionary').first().waitFor();
      await page.evaluate(()=>document.fonts.ready);
      const shot=async(name)=>{await page.screenshot({path:path.join(out,`${name}-${lang}.png`),animations:'disabled'});};
      await page.evaluate(async()=>{await window.galleryDemo.set({phase:'recording',live_text:window.galleryDemo.status().last_text,audio_level:0.13,audio_spectrum:Array.from({length:32},(_,i)=>0.2+((i*7)%17)/24)});});
      await page.locator('.live-transcript').waitFor();await shot('live');
      await page.evaluate(()=>window.galleryDemo.set({phase:'idle',live_text:''}));
      await button(ru?'Словарь':'Dictionary').first().click();
      await page.getByRole('button',{name:ru?/Личный словарь/:/Personal dictionary/}).click();
      await page.getByRole('heading',{name:ru?'Личный словарь':'Personal dictionary',exact:true}).waitFor();
      await shot('dictionary');
      await page.locator('.dictionary-scope').filter({hasText:'Atlas'}).click();
      await button(ru?'Найти термины':'Find terms').click();
      await page.getByText('AtlasClient',{exact:true}).click();
      await page.getByText('WorkspaceRole',{exact:true}).click();
      await shot('project');
      await page.keyboard.press('Escape');
      await button(ru?'Настройки':'Settings').click();
      await page.getByRole('heading',{name:ru?'Диктовка и вставка':'Dictation & delivery',exact:true}).evaluate(el=>el.closest('.settings-scroll').scrollTop=el.closest('.settings-section').offsetTop-el.closest('.settings-scroll').offsetTop-12);
      await shot('workflow');
      await page.evaluate(()=>window.galleryDemo.theme('light','blue'));
      await button(ru?'Начать тест':'Start test').click();
      await page.waitForFunction(()=>Number(document.querySelector('[role="meter"]').getAttribute('aria-valuenow'))>0);
      await page.getByRole('heading',{name:ru?'Микрофон и звук':'Microphone & sound',exact:true}).evaluate(el=>el.closest('.settings-scroll').scrollTop=el.closest('.settings-section').offsetTop-el.closest('.settings-scroll').offsetTop-12);
      await shot('audio');
      await button(ru?'Закрыть настройки':'Close settings').click();
      await shot('light');
      if(errors.length)throw new Error(errors.join('\n'));
      console.log(`Captured six ${lang} screenshots.`);
      await page.close();
    }
  } finally { await browser.close(); }
}
module.exports = { fixture };
if (require.main === module) captureGallery().catch(error=>{console.error(error);process.exitCode=1});
