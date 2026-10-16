// Types a real URL into the real app and runs a genuine analysis.
(async () => {
  const input = document.querySelector("[data-url-input]");
  if (!input) return "no input";
  input.value = "https://www.bilibili.com/video/BV1Fspj66E66/";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  const button = document.querySelector("[data-action='analyze']");
  if (!button) return "no analyze button";
  button.click();
  return "analyze clicked";
})()
