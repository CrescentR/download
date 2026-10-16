(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const [runtime, ffmpeg, settings] = await Promise.all([
    invoke('runtime_info'), invoke('check_ffmpeg'), invoke('get_settings'),
  ]);
  let dialogValidation;
  try {
    // Malformed options test the real ACL without opening an unattended dialog.
    await invoke('plugin:dialog|open', { options: { directory: 'invalid-test-value' } });
    throw new Error('Expected invalid options to be rejected');
  } catch (error) {
    dialogValidation = String(error);
    if (dialogValidation.includes('not allowed')) throw new Error(`Dialog permission still denied: ${error}`);
    if (!dialogValidation.includes('invalid type')) throw error;
  }
  document.querySelector('[data-action="open-settings"]').click();
  document.querySelector('[data-action="settings-tab"][data-tab="deps"]').click();
  return { desktop: true, runtimeMode: runtime.mode, ytdlpVersion: runtime.ytdlpVersion,
    ffmpegInstalled: ffmpeg.installed, ffmpegVersion: ffmpeg.version,
    template: settings.outputTemplate, dialogPermissionGranted: true,
    dialogArgumentValidation: dialogValidation };
})()
