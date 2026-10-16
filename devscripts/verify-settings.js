(async () => {
  const pause = () => new Promise(resolve => setTimeout(resolve, 80));
  const click = selector => {
    const element = document.querySelector(selector);
    if (!element) throw new Error(`Missing control: ${selector}`);
    element.click();
  };
  click('[data-action="open-settings"]');
  await pause();
  const measurements = [];
  for (const section of ['general', 'download', 'network', 'appearance', 'deps']) {
    click(`[data-action="settings-tab"][data-tab="${section}"]`);
    await pause();
    const rect = document.querySelector('.modal').getBoundingClientRect();
    measurements.push({ section, width: rect.width, height: rect.height });
  }
  if (measurements.some(item => item.width !== measurements[0].width || item.height !== measurements[0].height)) {
    throw new Error(`Settings resized between tabs: ${JSON.stringify(measurements)}`);
  }
  click('[data-action="settings-tab"][data-tab="general"]');
  const toggle = document.querySelector('[data-key="clipboardWatch"]');
  const before = toggle.getAttribute('aria-checked');
  toggle.click();
  if (toggle.getAttribute('aria-checked') === before) throw new Error('Toggle did not update visually');
  // Vite may timestamp module URLs after edits; use the exact loaded instance.
  const settingsUrl = performance.getEntriesByType('resource')
    .find(entry => new URL(entry.name).pathname.endsWith('/ui/settings.js'))?.name;
  const settings = await import(settingsUrl || '/src/ui/settings.js');
  settings.setDraftValue('outputDir', 'D:/测试目录');
  if (document.querySelector('[data-setting="outputDir"]').value !== 'D:/测试目录') throw new Error('Selected path is stale');
  click('[data-action="settings-tab"][data-tab="download"]');
  const quality = document.querySelector('[data-setting="quality"]');
  quality.value = 'audio';
  quality.dispatchEvent(new Event('change', { bubbles: true }));
  if (!document.querySelector('[data-setting="audioFormat"]')) throw new Error('Audio options did not appear');
  click('[data-action="settings-tab"][data-tab="network"]');
  if (!document.querySelector('[data-action="choose-cookie-file"]')) throw new Error('Cookie picker missing');
  click('[data-action="settings-tab"][data-tab="deps"]');
  click('[data-action="check-updates"][data-component="ffmpeg"]');
  await pause();
  click('[data-action="check-updates"][data-component="ytdlp"]');
  await pause();
  if (document.querySelectorAll('[data-action="install-update"]').length !== 2) throw new Error('Both components must offer updates');
  return { measurements, toggleUpdates: true, selectedPathUpdates: true, audioOptions: true, bothUpdateControls: true };
})()
