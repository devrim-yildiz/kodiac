// Codex · 2026-10-21 · Motion, accessibility and decoder continuity.
import { test, expect } from "@playwright/test";
test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill("ui-test-password-only");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Home", exact: true }),
  ).toBeVisible();
});
test("posters have restrained hover feedback and native shelf scrolling", async ({
  page,
}) => {
  const card = page.locator(".cinematic-shelf .title-card").first();
  const poster = card.locator(".poster-wrap");
  await card.hover();
  await expect
    .poll(() =>
      poster.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).m42),
    )
    .toBeLessThan(-3);
  await page.mouse.down();
  await expect
    .poll(() =>
      poster.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).m11),
    )
    .toBeCloseTo(0.98, 2);
  await page.mouse.move(0, 0);
  await page.mouse.up();
  await expect(page.locator(".cinematic-shelf").first()).toHaveCSS(
    "scroll-snap-type",
    "x",
  );
});
test("sheet reversal and settings leave the persistent decoder running", async ({
  page,
}) => {
  await page
    .getByRole("button", { name: "Resume Afterlight", exact: true })
    .click();
  const video = page.locator("#persistent-video");
  await expect
    .poll(() =>
      video.evaluate((v: HTMLVideoElement) => !v.paused && v.currentTime > 0),
    )
    .toBe(true);
  await video.evaluate((v) => {
    (window as any).__video = v;
    (window as any).__reloads = 0;
    v.addEventListener("loadstart", () => (window as any).__reloads++);
    v.addEventListener("emptied", () => (window as any).__reloads++);
  });
  await page
    .getByRole("button", { name: "Browse while playing", exact: true })
    .click();
  const before = await video.evaluate((v: HTMLVideoElement) => v.currentTime);
  const card = page.locator(".cinematic-shelf .title-card").first();
  await card.click();
  await expect(page.locator(".movie-sheet")).toBeVisible();
  await page.keyboard.press("Escape");
  // Re-target while the closing surface is still on screen.
  await card.dispatchEvent("click");
  await expect
    .poll(() =>
      page
        .locator(".movie-sheet")
        .evaluate((e) => Number(getComputedStyle(e).opacity)),
    )
    .toBe(1);
  await page.keyboard.press("Escape");
  await expect(page.locator(".movie-sheet")).toHaveCount(0);
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await expect(page.locator(".settings-page")).toBeVisible();
  await page.getByRole("button", { name: "Playback", exact: true }).click();
  await page
    .getByRole("button", { name: "Back to library", exact: true })
    .click();
  await expect(page.locator(".settings-page")).toHaveCount(0);
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime))
    .toBeGreaterThan(before);
  expect(await video.evaluate((v) => v === (window as any).__video)).toBe(true);
  expect(await page.evaluate(() => (window as any).__reloads)).toBe(0);
  expect(await video.evaluate((v: HTMLVideoElement) => v.paused)).toBe(false);
});
test("reduced motion cross-fades sheets without scaling or hover travel", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const card = page.locator(".cinematic-shelf .title-card").first();
  await card.hover();
  await expect(card).toHaveCSS("transform", "none");
  await expect(page.locator(".cinematic-shelf").first()).toHaveCSS(
    "scroll-behavior",
    "auto",
  );
  await card.click();
  await expect(page.locator(".movie-sheet")).toBeVisible();
  expect(
    await page
      .locator(".movie-sheet")
      .evaluate((e) =>
        e
          .getAnimations()
          .every((a) =>
            (a.effect as KeyframeEffect)
              .getKeyframes()
              .every((k) => k.scale === "1"),
          ),
      ),
  ).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.locator(".movie-sheet")).toHaveCount(0);
});
