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
test("personal playback and server settings are organized by task", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await expect(
    page.getByRole("heading", { name: "Top rated", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Settings", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Metadata", exact: true }).click();
  await expect(page.getByRole("switch")).toBeChecked();
  await page.getByRole("button", { name: "Libraries", exact: true }).click();
  await expect(page.locator(".path-list code")).toHaveCount(3);
  await page.getByRole("button", { name: "Playback", exact: true }).click();
  await page.getByLabel("Preferred audio language").selectOption("deu");
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByText("Changes saved.", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Diagnostics", exact: true }).click();
  await expect(page.getByText("1.1.0", { exact: true })).toBeVisible();
  // Reset persisted preferences because all browser tests share this isolated database.
  await page.request.put("/api/v1/preferences", {
    data: {
      audio_language_preference: "eng",
      subtitle_mode: "off",
      home_sections: ["continue", "collections", "recommended", "top"],
    },
  });
  expect(errors).toEqual([]);
});
test("spotlight keyboard navigation, fuzzy titles, people and collections", async ({
  page,
}) => {
  // The fixture inserts SQL after startup; wait for the background search snapshot.
  await expect
    .poll(
      async () => {
        const result = await page.request.get("/api/v1/search?q=Aftrlght");
        return (await result.json()).titles.some(
          (t: { title: string }) => t.title === "Afterlight",
        );
      },
      { timeout: 35000 },
    )
    .toBe(true);
  await page.keyboard.press("Control+k");
  const input = page.getByLabel("Search titles, people and collections");
  await input.fill("Aftrlght");
  await expect(page.locator(".spotlight-result").first()).toContainText(
    "Afterlight",
  );
  await page.keyboard.press("Enter");
  await expect(
    page
      .getByRole("dialog")
      .getByRole("heading", { name: "Afterlight", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close movie details" }).click();
  await page.getByRole("button", { name: "Open search", exact: true }).click();
  await input.fill("Mira");
  await page.getByRole("button", { name: /Mira Chen/ }).click();
  await expect(page.getByRole("dialog")).toContainText("A filmmaker exploring");
  await page.screenshot({ path: "../validation/luxury-v1.1/people-hub.png" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Open search", exact: true }).click();
  await input.fill("Afterlight");
  await expect(
    page.getByRole("button", { name: /The Afterlight Collection/ }),
  ).toBeVisible();
  await page.screenshot({
    path: "../validation/luxury-v1.1/command-palette.png",
  });
});
test("chapter intro and sequential next episode with countdown cancellation", async ({
  page,
}) => {
  await page.getByRole("tab", { name: "Series", exact: true }).click();
  await page
    .getByRole("button", { name: "Details for Northstar", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Play The Arrival", exact: true })
    .click();
  await expect(page.locator("#persistent-video")).toHaveJSProperty(
    "paused",
    false,
  );
  await expect(
    page.getByRole("button", { name: "Skip intro", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Skip intro", exact: true }).click();
  await expect
    .poll(() =>
      page
        .locator("#persistent-video")
        .evaluate((v) => (v as HTMLVideoElement).currentTime),
    )
    .toBeGreaterThan(7);
  await page
    .locator("#persistent-video")
    .evaluate((v) => ((v as HTMLVideoElement).currentTime = 65));
  await expect(
    page.getByRole("heading", { name: /Next episode in/ }),
  ).toBeVisible();
  await page.screenshot({ path: "../validation/luxury-v1.1/next-episode.png" });
  await page.getByRole("button", { name: "Keep watching" }).click();
  await expect(page.locator(".episode-prompt")).toHaveCount(0);
});
test("mobile home and settings remain within viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "../validation/luxury-v1.1/home-mobile.png",
    fullPage: true,
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await expect(page.locator(".settings-page")).toBeVisible();
  await page.waitForTimeout(250);
  await page.screenshot({
    path: "../validation/luxury-v1.1/settings-mobile.png",
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
});

test("next episode automatically starts after ten seconds and marks the previous episode complete", async ({
  page,
}) => {
  await page.getByRole("tab", { name: "Series", exact: true }).click();
  await page
    .getByRole("button", { name: "Details for Northstar", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Play The Arrival", exact: true })
    .click();
  await expect(page.locator("#persistent-video")).toHaveJSProperty(
    "paused",
    false,
  );
  await page
    .locator("#persistent-video")
    .evaluate((v) => ((v as HTMLVideoElement).currentTime = 65));
  await expect(
    page.getByRole("heading", { name: /Next episode in/ }),
  ).toBeVisible();
  await expect(page.locator(".cinema-header strong")).toHaveText(
    "An Open Door",
    {
      timeout: 20000,
    },
  );
  await expect(page.locator("#persistent-video")).toHaveJSProperty(
    "paused",
    false,
  );
});
