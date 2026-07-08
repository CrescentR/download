# Design QA

Reference: `C:/Users/Admin/AppData/Local/Temp/codex-clipboard-abdf67cb-6315-4011-8826-1ed3f56e2798.png`

Prototype: `http://127.0.0.1:1420/` during QA, then packaged with Tauri.

Checks:
- Layout follows the reference: top navigation, URL input row, quality selector, green download button, active downloads list, footer status bar.
- Chinese and English UI states were verified through the language toggle.
- FFmpeg missing state was verified with a localized warning banner and install/recheck actions.
- Settings panel was verified: gear opens settings, download path control exists, quality/language/runtime/dependency sections render.
- FFmpeg installer flow was changed to launch a separate terminal instead of blocking the Tauri command handler.
- Console check passed after adding the favicon and correcting icon registration.
- Responsive behavior keeps the queue usable on narrow screens.

Remaining notes:
- The preview mode shows two sample download rows to match the reference screenshot. Real Tauri runs render actual yt-dlp tasks.
- Thumbnail extraction is placeholder-like until the downloader pipeline surfaces real thumbnails.

Final result: passed
