// Opens the detail drawer for the running preview task.
(function () {
  const button = document.querySelector(".task [data-action='open-drawer']");
  if (!button) return "no drawer button";
  button.click();
  return "opened";
})()
