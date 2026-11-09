#!/usr/bin/env node
// Verify actual OHIF/Cornerstone rendering through an authenticated test gateway.
// Pair with dicom_pacs and DICOM_VIEWER_SESSION_FILE; synthetic fixtures only.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const sessionFile = process.argv[2];
assert(sessionFile, 'Usage: node scripts/test-dicom-viewer.mjs SESSION_JSON [OUTPUT_DIRECTORY]');
const output = path.resolve(process.argv[3] || path.dirname(sessionFile));
await mkdir(output, { recursive: true });
let session;
for (let attempt = 0; attempt < 600; attempt++) {
  try {
    session = JSON.parse(await readFile(sessionFile, 'utf8'));
    break;
  } catch (error) {
    if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error;
    await delay(500);
  }
}
assert(session, 'PACS test did not publish a viewer session');
const gateway = new URL(session.gateway_url);
assert.equal(gateway.hostname, '127.0.0.1', 'Only an isolated loopback test gateway is allowed');
assert(session.studies.every(study => study.uid.startsWith('1.2.826.0.1.3680043.10.999.')));
assert(/^[A-Za-z0-9._~-]+$/.test(session.access_token), 'Unexpected test credential encoding');
const port = Number(process.env.DICOM_OHIF_PORT || 3001);
const image = process.env.DICOM_OHIF_IMAGE || 'ohif/app:v3.13.14@sha256:40e509850ffaa45db50e64122387a8592b2b82965003568bd8f19f1cfb5ff2d3';
const config = {
  routerBasename: '/', showStudyList: true, extensions: [], modes: [],
  showWarningMessageForCrossOrigin: false, showCPUFallbackMessage: false,
  dataSources: [{
    namespace: '@ohif/extension-default.dataSourcesModule.dicomweb', sourceName: 'dicomweb',
    configuration: {
      friendlyName: 'OctoFHIR synthetic verification', name: 'pacs',
      qidoRoot: '/dicomweb/pacs', wadoRoot: '/dicomweb/pacs', wadoUriRoot: '/dicomweb/pacs',
      qidoSupportsIncludeField: true, imageRendering: 'wadors', thumbnailRendering: 'wadors',
      enableStudyLazyLoad: true, supportsFuzzyMatching: true, supportsWildcard: true,
    },
  }], defaultDataSourceName: 'dicomweb',
};
await writeFile(path.join(output, 'app-config.js'), `window.config = ${JSON.stringify(config)};\n`);
await writeFile(path.join(output, 'nginx.conf'), `events {}\nhttp {
  include /etc/nginx/mime.types;
  server {
    listen 80;
    root /usr/share/nginx/html;
    gzip_static on;
    add_header Cross-Origin-Opener-Policy same-origin always;
    add_header Cross-Origin-Embedder-Policy require-corp always;
    location = /app-config.js { gzip_static off; }
    location /dicomweb/ {
      proxy_set_header Authorization "Bearer ${session.access_token}";
      proxy_pass http://host.docker.internal:${gateway.port};
      proxy_buffering off;
    }
    location / { try_files $uri $uri/ /index.html; }
  }
}\n`, { mode: 0o600 });
const container = `octofhir-ohif-verification-${process.pid}`;
let browser;
const docker = args => execFileSync('docker', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
async function renderedPixels(page, canvas) {
  const png = await canvas.screenshot();
  return page.evaluate(async base64 => {
    const image = new Image();
    image.src = `data:image/png;base64,${base64}`;
    await image.decode();
    const canvas = document.createElement('canvas');
    canvas.width = image.width; canvas.height = image.height;
    const ctx = canvas.getContext('2d'); ctx.drawImage(image, 0, 0);
    const width = Math.floor(image.width * .4), height = Math.floor(image.height * .4);
    const { data } = ctx.getImageData(Math.floor(image.width * .3), Math.floor(image.height * .3), width, height);
    const levels = new Set();
    let top = 0, bottom = 0, topCount = 0, bottomCount = 0;
    for (let i = 0; i < data.length; i += 4) {
      levels.add(data[i]);
      if (Math.floor(i / 4 / width) < height / 2) { top += data[i]; topCount++; }
      else { bottom += data[i]; bottomCount++; }
    }
    return { min: Math.min(...levels), max: Math.max(...levels), levels: levels.size,
      verticalDelta: bottom / bottomCount - top / topCount };
  }, png.toString('base64'));
}
try {
  docker(['run', '-d', '--user', '0:0', '--name', container, '-p', `127.0.0.1:${port}:80`,
    '-v', `${path.join(output, 'nginx.conf')}:/etc/nginx/nginx.conf:ro`,
    '-v', `${path.join(output, 'app-config.js')}:/usr/share/nginx/html/app-config.js:ro`,
    '--entrypoint', 'nginx', image, '-g', 'daemon off;']);
  let ready = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    try {
      ready = (await fetch(`http://127.0.0.1:${port}/app-config.js`)).ok;
      if (ready) break;
    } catch { /* Docker has not published the listener yet. */ }
    await delay(200);
  }
  if (!ready) execFileSync('docker', ['logs', container], { stdio: 'inherit' });
  assert(ready, 'OHIF container did not become ready');
  const { chromium } = await import(process.env.DICOM_PLAYWRIGHT_MODULE || 'playwright-core');
  browser = await chromium.launch({ channel: 'chrome', headless: true,
    args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  const results = [];
  for (const study of session.studies) {
    const page = await browser.newPage({ viewport: { width: 1400, height: 1000 } });
    const failures = [], frames = [], errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    page.on('response', response => {
      const url = new URL(response.url());
      if (!url.pathname.startsWith('/dicomweb/')) return;
      if (response.status() >= 400) failures.push({ path: url.pathname, status: response.status() });
      if (url.pathname.includes('/frames/')) frames.push({ path: url.pathname, status: response.status() });
    });
    try {
      await page.goto(`http://127.0.0.1:${port}/viewer?StudyInstanceUIDs=${study.uid}`);
      await page.waitForFunction(() => [...document.querySelectorAll('canvas')]
        .some(canvas => canvas.width > 10 && canvas.height > 10), null, { timeout: 90000 });
      let pixels;
      const canvas = page.locator('canvas').first();
      for (let attempt = 0; attempt < 90; attempt++) {
        pixels = await renderedPixels(page, canvas);
        if (pixels.max - pixels.min > 40 && pixels.levels > 16 && frames.length) break;
        await delay(500);
      }
      assert(frames.some(frame => frame.status === 200), 'OHIF did not retrieve a frame');
      assert.equal(failures.length, 0, `Gateway errors: ${JSON.stringify(failures)}`);
      assert.equal(errors.length, 0, `Viewer errors: ${JSON.stringify(errors)}`);
      assert(pixels.max - pixels.min > 40 && pixels.levels > 16,
        `OHIF did not render the synthetic gradient: ${JSON.stringify(pixels)}`);
      const skipTour = page.getByText('Skip all', { exact: true });
      if (await skipTour.isVisible()) await skipTour.click();
      const confirm = page.getByRole('button', { name: 'Confirm and hide' });
      if (await confirm.isVisible()) await confirm.click();
      let secondFramePixels;
      if (study.kind === 'multiframe') {
        await canvas.hover();
        await page.mouse.wheel(0, 120);
        for (let attempt = 0; attempt < 90; attempt++) {
          secondFramePixels = await renderedPixels(page, canvas);
          if (secondFramePixels.verticalDelta * pixels.verticalDelta < 0) break;
          await delay(500);
        }
        assert(frames.some(frame => frame.path.endsWith('/frames/2') && frame.status === 200));
        assert(secondFramePixels.verticalDelta * pixels.verticalDelta < 0,
          'Scrolling did not render the reversed gradient in the second frame');
      }
      await page.screenshot({ path: path.join(output, `ohif-${study.kind}.png`) });
      results.push({ ...study, frames, pixels, secondFramePixels, status: 'passed' });
      console.log(`${study.kind}: OHIF rendered ${pixels.levels} intensity levels through the gateway`);
    } catch (error) {
      await page.screenshot({ path: path.join(output, `ohif-${study.kind}-failed.png`) });
      console.error((await page.locator('body').innerText()).slice(-4000));
      console.error(JSON.stringify({ failures, errors, frames }));
      throw error;
    } finally { await page.close(); }
  }
  await writeFile(path.join(output, 'ohif-results.json'), JSON.stringify({ image, browser: browser.version(), transport: session.transport, ups: session.ups, results }, null, 2));
  await writeFile(`${sessionFile}.done`, JSON.stringify({ session_id: session.session_id, status: 'passed' }));
} finally {
  if (browser) await browser.close();
  try { docker(['rm', '-f', container]); } catch { /* Container already removed. */ }
}
