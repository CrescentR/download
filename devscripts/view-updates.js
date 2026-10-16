// Opens Settings on the Runtime tab and runs an update check.
(async () => {
  document.querySelector("[data-action='open-settings']")?.click();
  await new Promise((resolve) => setTimeout(resolve, 500));
  document.querySelector("[data-action='settings-tab'][data-tab='deps']")?.click();
  await new Promise((resolve) => setTimeout(resolve, 500));
  document.querySelector("[data-action='check-updates']")?.click();
  await new Promise((resolve) => setTimeout(resolve, 900));
  return {
    heading: document.querySelector(".settings-panes h3")?.innerText || null,
    buttons: Array.from(document.querySelectorAll(".settings-panes button")).map((b) =>
      b.innerText.replace(/\s+/g, " ").trim(),
    ),
    panels: Array.from(document.querySelectorAll(".dep-panel")).map((p) =>
      p.innerText.replace(/\s+/g, " ").trim().slice(0, 90),
    ),
  };
})()
