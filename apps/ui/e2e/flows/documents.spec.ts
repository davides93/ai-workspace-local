import { expect, openApp, test } from "../support/app";

test("documents: add a file to a collection and search for it", async ({ page }) => {
  await openApp(page, "/documents");
  const list = page.getByRole("region", { name: "Documenti della raccolta" });
  await expect(list.getByText("contratto-affitto.txt")).toBeVisible();

  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Scegli file" }).click();
  await (await chooser).setFiles({
    name: "garanzia-caldaia.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("La caldaia Termo X ha una garanzia di cinque anni dalla data di installazione. Il tagliando annuale è obbligatorio."),
  });
  await expect(page.getByRole("list", { name: "Operazioni" }).getByText("1 documento aggiunto.")).toBeVisible();
  await expect(list.getByText("garanzia-caldaia.txt")).toBeVisible();

  const search = page.getByRole("region", { name: "Cerca nei documenti" });
  const query = search.getByRole("searchbox", { name: "Cosa cerchi?" });
  await expect(search.getByRole("button", { name: "Cerca", exact: true })).toBeDisabled();
  await query.fill("garanzia caldaia");
  await search.getByRole("button", { name: "Cerca", exact: true }).click();
  await expect(search.getByText("garanzia-caldaia.txt")).toBeVisible();
  await expect(search.getByText(/garanzia di cinque anni/)).toBeVisible();
});
