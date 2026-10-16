// Opens the settings modal on the Download tab.
(function () {
  const button = document.querySelector("[data-action='open-settings']");
  if (!button) return "no settings button";
  button.click();
  return "opened";
})()
