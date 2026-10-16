// Runs an analysis against a playlist-shaped URL.
(async () => {
  const input = document.querySelector("[data-url-input]");
  input.value = "https://www.youtube.com/playlist?list=PLtest123";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  document.querySelector("[data-action='analyze']").click();
  await new Promise((resolve) => setTimeout(resolve, 900));
  return "playlist analyzed";
})()
