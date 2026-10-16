// Clears the queue to exercise the empty state.
(async () => {
  const button = document.querySelector("[data-action='clear-finished']");
  if (button) button.click();
  await new Promise((resolve) => setTimeout(resolve, 400));
  return "cleared";
})()
