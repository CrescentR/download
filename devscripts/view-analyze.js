// Fills the composer and runs an analysis, to exercise the preview panel.
(async () => {
  const input = document.querySelector("[data-url-input]");
  if (!input) return "no input";
  input.value = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  document.querySelector("[data-action='analyze']").click();
  await new Promise((resolve) => setTimeout(resolve, 900));
  return "analyzed";
})()
