// Records docs/motion/index.html frame by frame into docs/media/ (MP4 + GIF).
// Usage (from this folder): npm install && node record.mjs
// Needs Google Chrome (or set CHROME=path) and ffmpeg on PATH.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import puppeteer from "puppeteer-core";

const here = dirname(fileURLToPath(import.meta.url));
const page_ = resolve(here, "..", "index.html");
const media = resolve(here, "..", "..", "media");
const frames = join(here, "frames");
const FPS = Number(process.env.FPS || 30);

const chrome =
  process.env.CHROME ||
  ["C:/Program Files/Google/Chrome/Application/chrome.exe", "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", "/usr/bin/google-chrome", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"].find(existsSync);
if (!chrome) throw new Error("Chrome not found; set CHROME=path/to/chrome");

rmSync(frames, { recursive: true, force: true });
mkdirSync(frames, { recursive: true });
mkdirSync(media, { recursive: true });

const browser = await puppeteer.launch({ executablePath: chrome, headless: true, protocolTimeout: 60000, args: ["--hide-scrollbars"] });
const page = await browser.newPage();
await page.setViewport({ width: 1400, height: 900, deviceScaleFactor: 1 });
await page.goto(pathToFileURL(page_).href, { waitUntil: "load" });
// wait for the web fonts (return a string: a FontFaceSet does not serialize)
await page.evaluate(async () => (await document.fonts.ready).status);
// make the stage exactly 1280x720 (scale 1)
await page.addStyleTag({ content: ".wrap{max-width:none!important} .stage{width:1280px!important;border-radius:0!important;border:0!important}" });
// let the page's own autoplay start, then take over the timeline
await new Promise((r) => setTimeout(r, 1200));
const duration = await page.evaluate(() => {
  window.__tour.pause(0);
  return window.__tour.duration();
});
const stage = await page.$("#stage");
const box = await stage.boundingBox();
const clip = { x: Math.round(box.x), y: Math.round(box.y), width: 1280, height: 720 };

const total = Math.ceil(duration * FPS);
console.log(`recording ${total} frames (${duration.toFixed(1)}s @ ${FPS}fps)`);
for (let i = 0; i < total; i++) {
  for (let attempt = 0; ; attempt++) {
    try {
      await page.evaluate((t) => {
        window.__tour.pause();
        window.__tour.seek(t, false);
        return true;
      }, i / FPS);
      await page.screenshot({ path: join(frames, `f${String(i).padStart(5, "0")}.png`), clip });
      break;
    } catch (e) {
      if (attempt >= 2) throw e;
      console.log(`  frame ${i}: retrying (${e.message})`);
    }
  }
  if (i % (FPS * 5) === 0) console.log(`  ${(i / FPS).toFixed(0)}s`);
}
await browser.close();

const ff = (args) => {
  const r = spawnSync("ffmpeg", ["-y", "-loglevel", "error", ...args], { stdio: "inherit" });
  if (r.status !== 0) throw new Error("ffmpeg failed");
};
const input = ["-framerate", String(FPS), "-i", join(frames, "f%05d.png")];
// full-quality video
ff([...input, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-movflags", "+faststart", join(media, "smartftp-tour.mp4")]);
// README preview: smaller GIF with an optimized palette
const gifFilter = "fps=12,scale=880:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=160:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle";
ff([...input, "-vf", gifFilter, join(media, "smartftp-tour.gif")]);
rmSync(frames, { recursive: true, force: true });
console.log("wrote docs/media/smartftp-tour.mp4 and docs/media/smartftp-tour.gif");
