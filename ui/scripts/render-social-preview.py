"""Render ui/scripts/social-preview.html to ui/public/branding/social-preview.png (GitHub social preview)."""
from pathlib import Path

from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
OUT = HERE.parent / "public" / "branding" / "social-preview.png"

with sync_playwright() as p:
    browser = p.chromium.launch()
    page = browser.new_page(viewport={"width": 1280, "height": 640}, device_scale_factor=1)
    page.goto((HERE / "social-preview.html").as_uri(), wait_until="networkidle")
    page.evaluate("document.fonts.ready")
    page.screenshot(path=str(OUT))
    browser.close()
print(OUT)
