// Switches the interface to English via the sidebar toggle.
(async () => {
  const button = document.querySelector("[data-action='toggle-lang']");
  if (!button) return "no language button";
  button.click();
  await new Promise((resolve) => setTimeout(resolve, 700));
  return document.documentElement.lang || "toggled";
})()
