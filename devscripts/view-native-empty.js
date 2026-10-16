(async () => {
  const wait = () => new Promise(resolve => setTimeout(resolve, 140));
  // Remove preview tasks through the same controls as the app, so the empty
  // screen reflects actual task state rather than a presentation-only flag.
  for (let attempt = 0; attempt < 12 && document.querySelector('.task'); attempt++) {
    const row = document.querySelector('.task');
    const button = row.querySelector('[data-action="remove"], [data-action="cancel"]');
    if (!button) throw new Error('Preview task cannot be removed');
    button.click();
    await wait();
  }
  if (document.querySelector('.task') || !document.querySelector('.empty')) {
    throw new Error('Empty queue did not render automatically');
  }
  return 'Empty queue rendered automatically after removing preview tasks';
})()
