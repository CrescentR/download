# Yuki third-party components

Yuki follows the repository's existing **Unlicense** (`../LICENSE`). The upstream
yt-dlp license and notices in the repository have not been replaced. Dependencies
retain their own licenses; the application license does not relicense them.

## Tools downloaded separately

Installers contain the GUI only, without yt-dlp, FFmpeg, ffprobe, Python or a JS
runtime. Downloads go directly from upstream HTTPS releases into user storage.
Network access is needed to install missing tools. Existing system tools can be
used without downloading them.

* **yt-dlp**: https://github.com/yt-dlp/yt-dlp . Its source is Unlicense;
  official standalone/PyInstaller executables include additional components and
  are distributed under **GPLv3+**. See the exact release's README, license and
  corresponding source at https://github.com/yt-dlp/yt-dlp#license .
* **FFmpeg / ffprobe**: https://ffmpeg.org/legal.html . Windows and Linux managed
  downloads select BtbN's **LGPL shared** builds:
  https://github.com/BtbN/FFmpeg-Builds . Build recipes and source instructions
  are provided by that project. The extracted tree keeps upstream license files
  and libraries, plus a `YUKI-SOURCE.txt` recording the downloaded URL and build.
  The exact build's configuration and included libraries determine its terms.
* macOS FFmpeg currently uses an existing installation or Homebrew through the
  installation guide. BtbN does not provide macOS assets; Yuki does not select
  an incompatible Windows/Linux archive as a fallback.

Runtime download is a packaging choice, **not a blanket license exemption**.
Anyone redistributing binaries, hosting a mirror, or prebundling tools must
review the exact binaries' license notices and corresponding-source obligations.
Changing to bundled dependencies requires updating this notice and release flow.

## Application dependencies

The installer includes a small **Unlicense** yt-dlp Douyin extractor adapter,
adapted from viking1304's public-domain/Unlicense contribution in
https://github.com/yt-dlp/yt-dlp/pull/17826 . It registers an anonymous Douyin
visitor session and signs the detail request. Attribution is retained in
`yt-dlp-plugins/douyin-compat/yt_dlp_plugins/extractor/yuki_douyin.py`.
No yt-dlp executable is included in the installer.

The interface bundles Inter, Noto Sans SC, and JetBrains Mono from Fontsource.
These fonts use the SIL Open Font License 1.1. Their license texts are shipped in
`licenses/fonts/`. Font assets are self-hosted; no font CDN is contacted at runtime.
Tailwind CSS and its Vite plugin (build tools) use the MIT license.

Tauri and its official dialog/opener plugins: MIT OR Apache-2.0.
Serde / serde_json / regex / thiserror: MIT OR Apache-2.0.
sha2: MIT OR Apache-2.0. uuid: MIT OR Apache-2.0.
ureq: MIT OR Apache-2.0. zip: MIT. tar / xz2: MIT OR Apache-2.0.
Vite (build tool): MIT. Transitive dependencies retain their own notices.

Lockfiles record exact versions. Before public binary redistribution, review
the complete dependency inventory (`cargo metadata --locked` and
`npm ls --all`) and upstream license files, including native platform libraries.

## Implementation references

Architecture was compared with [DownVid](https://github.com/yxxbc/downvid) and
[Videdown](https://github.com/cshuangyy/videdown), both MIT. The fixes here use
this project's Tauri/Rust implementation; their Electron/Go code is not vendored.
