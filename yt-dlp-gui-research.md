# Research: two yt-dlp desktop GUIs + bundling yt-dlp/FFmpeg in a Tauri app

Researched 2026-10-15. Method: GitHub REST API, raw source files from both repos, yt-dlp official README/source, ffmpeg.org, gyan.dev, BtbN, Tauri docs. Every claim below is backed by a source URL. Items I could not verify are marked **[UNVERIFIED]**.

---

## 0. Headline finding: the two projects are the same lineage

`downvid`'s own README states: *"DownVid started from [Videdown] and has since been substantially modified, refactored and extended."*

So this is not a comparison of two independent designs — it is **before/after a deliberate rewrite** of the same app. Videdown is the ancestor (1931 stars, last push 2026-08-22); DownVid is the evolved fork (776 stars, last push 2026-10-14). Several of DownVid's code comments explicitly name the Videdown-era bugs they fix, which makes it an unusually well-documented source of "what goes wrong" for exactly the app you are building.

Both repos exist and are public (HTTP 200). Both are **Electron + Vue 3 + TypeScript** — **neither is Tauri**.

| | videdown | downvid |
|---|---|---|
| Repo | https://github.com/cshuangyy/videdown | https://github.com/yxxbc/downvid |
| Stars / size | 1931 / 890 KB | 776 / 69 MB |
| Created / last push | 2026-04-14 / 2026-08-22 | 2026-08-22 / 2026-10-14 |
| Version | v1.2.7 | v1.3.4 |
| License | MIT | MIT |
| UI stack | Electron 30.0.1, Vue 3.4, TS 5.2, Tailwind 3, Vite 5 | Electron 44.5.1, Vue 3.5, TS 5.9, Tailwind 4, Vite 8, Pinia, Vitest |
| Main process | one 90,960-byte `electron/main.ts` (2,756 lines) | `electron/` split into ipc/, download/, parsers/, utils/ + a **Go sidecar** |
| yt-dlp | bundled `.exe` in installer | **not bundled**; detected or downloaded at runtime |
| FFmpeg | bundled `ffmpeg.exe` only (no ffprobe) | **not bundled**; downloads ffmpeg+ffprobe at runtime |
| GitHub Actions | **0 workflows** | **11 workflows** |

---

## 1. videdown (github.com/cshuangyy/videdown)

### A. Stack & architecture
- **Electron + Vue 3 + TypeScript** (`electron-builder.json5`, `electron/main.ts`, `src/`). Sources: https://github.com/cshuangyy/videdown/blob/main/package.json , https://github.com/cshuangyy/videdown/blob/main/README.md
- **yt-dlp: BUNDLED as a sidecar.** `electron-builder.json5` has `extraResources` copying `./yt-dlp.exe` → `./yt-dlp.exe`, plus `asarUnpack` for `**/yt-dlp.exe`. Resolution in `getYtDlpPath()` probes `APP_ROOT/yt-dlp.exe`, `process.resourcesPath/yt-dlp.exe`, `__dirname/../../yt-dlp.exe`, then falls back to the literal string `'yt-dlp.exe'`.
  - The filename is **hardcoded with the `.exe` extension and a Windows name**. On macOS/Linux the fallback `'yt-dlp.exe'` cannot resolve a `yt-dlp` installed via Homebrew/apt, so those platforms have no working path. (Reading of code, not documented by the project.)
  - README tells macOS/Linux users to `brew install yt-dlp ffmpeg` / `apt install yt-dlp ffmpeg` — i.e. **user must install** on non-Windows.
- **FFmpeg: BUNDLED, but only `ffmpeg.exe` — no ffprobe.** `getFfmpegPath()` probes `APP_ROOT`, `resourcesPath`, `__dirname/../..`, `cwd`, then bare `ffmpeg` (PATH fallback). README's Windows source-build instructions download `ffmpeg-master-latest-win64-gpl.zip` from BtbN and copy out only `bin/ffmpeg.exe`.
- **Python needed? No** for end users (bundles the PyInstaller `yt-dlp.exe`). Developers need nothing Python-side either.
- **JS runtime: user must install it.** `checkJsRuntime()` probes bare `node` / `node.exe` then hardcoded `C:\Program Files\nodejs\node.exe`, `C:\Program Files (x86)\nodejs\node.exe`, `D:\NodeJs\node.exe`, `E:\NodeJs\node.exe`, then Deno names; if absent it shows a dialog "需要安装 Node.js 运行时" and opens `https://nodejs.org/zh-cn/download/package-manager`. So YouTube support depends on a user-installed Node.
- **Also requires Chrome** for Douyin/Kuaishou (uses `puppeteer-core`; `promptChromeDownload()` opens google.com/chrome).

### B. Feature set
- **Format listing before download: YES.** `ytdlp:parse` runs `--dump-json` (non-YouTube) or `--print '%()j'` (YouTube, "to get the complete format list including m3u8 multi-audio"), parses `info.formats`, filters to `vcodec !== 'none' && height > 0`, excludes m3u8, estimates missing sizes from `tbr * duration / 8`.
- **Progress parsing: regex on human-readable output.** Uses `--newline` and then matches three regexes against the text, e.g.
  `/\[download\]\s+(\d+\.?\d*)%\s+of\s+~?\s*(\d+\.?\d*\w?)\s+at\s+([\d.]+\w?\/s)\s+ETA\s+(\d+:\d+)/`
  plus a fallback `/\[download\]\s+(\d+\.?\d*)%/`, plus a crude `/(\d+\.?\d*)%/` on **stderr**. Final path is scraped with `\[download\] Destination: (.+)`, `\[download\] (.+) has already been downloaded`, `\[Merger\] Merging formats into "(.+)"`. **No `--progress-template` is used anywhere.**
- Cookies: **yes** — `--cookies <file>` or `--cookies-from-browser <browser>`, but **only applied for YouTube and Bilibili URLs** ("B站也需要 cookies 避免 412 错误"). README documents the "Get cookies.txt" extension workflow.
- Proxy: **NO.** A grep for `proxy` across the whole 90 KB main process returns zero matches.
- Subtitles: **yes** — `--write-subs --sub-langs <list> --convert-subs srt`, both alongside video and as a subtitle-only mode (`--skip-download`). Subtitle-only file path is guessed/constructed rather than read from yt-dlp.
- Playlists: **NO** — every invocation passes `--no-playlist`.
- Batch URLs: **NO** — no multi-URL parsing in the main process.
- Queue / concurrency: **task list UI only, no concurrency limit.** `DownloadView.vue` calls `processDownload(task)` directly for each new task; there is no gate and no cap, so concurrent downloads run unbounded in parallel.
- Other: format selector `${formatId}+bestaudio[ext=m4a]/bestaudio/best`; `-x --audio-format mp3 --audio-quality 0` for audio; `--merge-output-format mp4`; `--js-runtimes <kind>:<path>`; `--ffmpeg-location getFfmpegPath()` — **a file path, not a directory**.
- UI adds spoofed `--user-agent` and `--add-header Accept-Language/Accept`, plus `--no-check-certificates`.

### C. Update mechanism
- **App self-update: NO.** There is a manual IPC handler `app:checkForUpdates` that GETs `https://api.github.com/repos/cshuangyy/videdown/releases/latest`, compares `tag_name` to `app.getVersion()`, and returns `hasUpdate` + `html_url`. The user must download the installer manually.
  - `electron-updater@^6.8.3` **is declared in package.json dependencies but is never imported or called** — a grep for `autoUpdater|electron-updater|quitAndInstall` in `main.ts` returns nothing. So auto-update is not wired up despite the dependency.
- **yt-dlp version check: NONE.** No version comparison, no `-U`, no manifest of pinned versions anywhere.

### D. Packaging / release
- `electron-builder.json5` declares mac `dmg`, win `nsis` (x64), linux `AppImage`. Note `package.json` *also* contains a `build` block (win nsis only) — two configs coexist; I did not verify which one electron-builder actually selects. **[UNVERIFIED]** which config wins.
- **Actual published artifacts: only one per release.** GitHub API shows v1.2.7 (2026-08-22) with exactly **1 asset: `Videdown.Setup1.2.7.exe`, 117.4 MB, 10,890 downloads**. v1.2.1: 1 asset. v1.2.0: 1 asset. No macOS or Linux artifact has ever been published, despite the config declaring those targets.
- **CI: none.** `GET /repos/cshuangyy/videdown/actions/workflows` → `total_count: 0`. The repository tree contains no `.github` directory at all. The installer is built locally by the maintainer.

---

## 2. downvid (github.com/yxxbc/downvid)

### A. Stack & architecture
- **Electron 44.5.1 + Vue 3.5 + TypeScript + Tailwind 4 + Vite 8 + Pinia + Vitest**, `electron-updater` 6.8.3, `puppeteer-core`.
- **Plus a Go 1.24 sidecar** (`backend/cmd/downvid-deps`, standard library only — the repo has a 49-byte `go.mod` and **no `go.sum`**). It is built by a custom Vite plugin `downvid-go-backend` which runs
  `go build -o backend/build/<platform>-<arch>/downvid-deps ./cmd/downvid-deps` with `GOOS`/`GOARCH`/`CGO_ENABLED=0` at `buildStart`, then electron-builder `extraResources` copies `backend/build/${platform}-${arch}` → `resources/downvid-deps`.
  - Architecture: `Vue 3 renderer <--IPC--> Electron main <--NDJSON--> Go sidecar`, and main process spawns `yt-dlp`/`FFmpeg` directly.
- **yt-dlp: NOT bundled. Downloaded at runtime, pinned and hash-verified.**
  - electron-builder comment: *"安装包不再内置 yt-dlp / ffmpeg：只带 Go 边车，运行时识别并复用系统已安装的版本，缺失时由边车下载到 userData"*, and in `electron/utils/binary.ts`: *"以前这里会去 process.resourcesPath/bin 找随包分发的二进制；现在安装包不再携带"*.
  - Source: **`yt-dlp/yt-dlp-nightly-builds`**, pinned tag **`2026.10.10.233141`** (nightly, *not* stable). Assets per platform: `win32/x64 → yt-dlp.exe`, `darwin/* → yt-dlp_macos`, `linux/x64 → yt-dlp_linux`, `linux/arm64 → yt-dlp_linux_aarch64`. SHA-256 taken from upstream **`SHA2-256SUMS`**.
  - Rationale in code: nightly is chosen because "站点修复更及时" (site fixes land sooner); the tag is pinned rather than `latest` so installs are reproducible and verifiable byte-for-byte.
- **FFmpeg: NOT bundled. Downloaded at runtime.** Source: **`eugeneware/ffmpeg-static`**, pinned tag **`b6.1.1`** (= FFmpeg **6.1.1**). Both **`ffmpeg` and `ffprobe`** are installed, as `<name>.gz` files that are gunzipped after hash verification. Upstream publishes no checksums, so the project computes and pins its own SHA-256. Code comment justifies it over BtbN: smaller, single gzip file, and includes `libmp3lame / aac / libx264` for audio conversion and muxing.
  - **Note:** FFmpeg 6.1.1 is ~2 major versions behind (current is 9.0.2), and eugeneware/ffmpeg-static's own README says the ffmpeg version currently used is 6.1.1 — that repo has not refreshed.
- **Python: not needed at all** (uses the standalone builds).
- **JS runtime: still required for YouTube, and still user-supplied.** `electron/utils/js-runtime.ts` requires Node 22+ or Deno 2.3+; if missing, `promptNodeDownload()` opens nodejs.org. DownVid passes an **explicit** runtime kind (`--js-runtimes node:<path>`), with the comment that guessing from the path mislabels Electron's own binary as deno ("实测会拉起一整个应用").
- **Dependency resolution order** (documented in `binary.ts`): manual override → managed dir under userData → **login shell's PATH** → platform common install dirs → bare command name. Status is cached 60 s; the Go sidecar's detect timeout is 80 s.
- **Managed install location:** `path.join(app.getPath('userData'), 'runtime', 'bin')`.
- Sidecar install post-processing on macOS: strip `com.apple.quarantine` and apply an **ad-hoc codesign** (`codesign --force --deep --sign -`), because the downloaded binaries are unsigned/quarantined.

### B. Feature set
- **Format listing: YES** — `--dump-json` then `buildVideoFormats()` maps `info.formats` to UI rows (dedupes by quality+fps keeping highest tbr, drops m3u8 when a direct link exists at the same height, falls back to `bv*+ba/b` if nothing parses). Cached in memory for 30 min (`PARSE_CACHE_VERSION = 5`).
- **Progress parsing: STRUCTURED via `--progress-template` — this is the key improvement over videdown.**
  - Custom ASCII prefixes `[dvp]`, `[dvpp]`, `[dvfile]`, `[dvsubs]`, dispatched with `line.startsWith(...)` — **no regex on human text**.
  - Flags: `--newline --progress --progress-template "download:[dvp]%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(progress.fragment_index)s|%(progress.fragment_count)s"` and `--progress-template "postprocess:[dvpp]%(progress.postprocessor)s|%(progress.status)s"`.
  - Final path: `--print "after_move:[dvfile]%(filepath)s"`; subtitles: `--print "after_move:[dvsubs]%(requested_subtitles)j"` (JSON). Comment notes `--print` implies `--quiet`, hence the explicit `--progress`.
  - Merger/ExtractAudio postprocessor events drive a "merging" UI state at 99%.
  - stdout and stderr are both read with a proper line-buffering reader; stderr is truncated to the last 20,000 chars for error extraction; aria2c progress is parsed by a separate function.
- Cookies: **yes, and the most sophisticated part of either codebase.** Supports `chrome, edge, firefox, safari, brave, chromium`. Because `--cookies-from-browser` fails while the browser is running (Windows Chrome DB locking / App-Bound encryption), it **exports the decrypted cookies to a temp file and passes BOTH `--cookies-from-browser <b>` AND `--cookies <path>`**, then replays attempts across browsers and produces targeted failure hints. Also supports a user-imported `cookies.txt`.
- Proxy: **yes** — `--proxy` is added to yt-dlp runs when set, and forwarded to the Go sidecar as `fetch --proxy` for dependency downloads.
- Subtitles: **yes** — `--write-subs --sub-langs <list> --convert-subs srt`, with a dedicated subtitle-only mode; also YouTube multi-audio-track selection.
- Playlists: **NO** — `--no-playlist --playlist-items 1` on every run.
- Batch URLs: **NO** — `UrlInput.vue` is a single `<textarea>` bound to one `store.url` string.
- **Queue / concurrency: YES, capped at 3.** `src/stores/download-rules.ts` exports `export const MAX_CONCURRENT_DOWNLOADS = 3`. All start paths funnel through a single gate `pumpQueue()`; `selectStartableTasks()` picks FIFO by `createdAt`; `'merging'` also occupies a slot (it holds ffmpeg); `processDownload` has a `taskExecutions` re-entrancy guard. The code comment states the exact bug it fixes: previously `startDownload()` called `processDownload` directly for every task, so repeated clicks ran many downloads at once, each opening 8–16 chunk requests, all competing for bandwidth. It explains 3 rather than 1: one task is limited by single-site throttling, but more than 3 multiplies concurrent fragment requests for diminishing returns and higher 429 risk.
- **Download acceleration (beyond either project's README spin):**
  - `--http-chunk-size 10M` — with a measured claim in-code: same 17.6 MB file took 109 s single-connection (0.16 MB/s) vs 3–5 s with chunking (~5 MB/s), **>30×**. Notes this works where `--concurrent-fragments` does not, because many sites serve a single mp4 direct link rather than HLS/DASH fragments.
  - `--concurrent-fragments <N>` for HLS/DASH (default 8, configurable up to 16).
  - Optional **aria2c** via `--downloader "http:<aria2c>"` + `--downloader dash,m3u8:native`, with `--downloader-args` (`--split`, `--max-connection-per-server`, `--min-split-size=1M`, `--file-allocation=none`, …) and `--paths home:<out> --paths temp:<out>/.downvid-aria2` to isolate aria2's sparse segments so a resume can't be mistaken for contiguous data by the native downloader.
- **`--load-info-json` reuse:** the parse step writes yt-dlp's info JSON to a temp file; the download step reuses it (TTL 20 min) to skip a second extraction, and falls back to re-extracting by URL if it fails before the download starts (e.g. expired direct link).
- Additional engineering worth stealing: `--ignore-config` (so a user's global yt-dlp config can't change behavior), `--socket-timeout 10`, `--extractor-retries 1`, per-URL extractor-arg fallbacks (e.g. X/Twitter `twitter:api=syndication`), YouTube retry state machine, a version gate, and structured machine-readable error markers embedded in human messages (`[[deps-missing:ffmpeg]]`, `[[parse-error:<kind>]]`) that the renderer parses.
- **Minimum yt-dlp version gate:** `MinYtDlpVersion()` returns the pinned manifest tag exactly. The code comment explains why a year-based check is wrong: yt-dlp uses **date versions** and ships site fixes weekly; measured, the **stable `2026.08.19` build fails on YouTube with `Requested format is not available`** (upstream issue #17586 fixed in a newer build), yet a year-based check would pass it — leaving users stuck on a dead version. This is direct evidence for building on **nightly**.

### C. Update mechanism — two independent tracks
1. **yt-dlp/FFmpeg: manifest baked into the app, refreshed in CI, gated at release.**
   - Tags + SHA-256 live in a **generated Go file** `backend/internal/manifest/assets.gen.go`, produced by `scripts/gen-deps-manifest.mjs` (also `pnpm deps:manifest`).
   - There is **no runtime check against upstream**. The UI compares the found/managed binary's version against `requiredVersion` from the baked manifest and only shows an "Update" button when the managed copy is strictly older (`stale`), to avoid making users re-download ~37 MB for an identical version.
   - `.github/workflows/update-deps.yml` is **`workflow_dispatch` only (manual, no cron)**; it re-fetches upstream tags/hashes, runs gofmt/vet/test, and commits the regenerated `assets.gen.go`.
   - **The clever part:** `pnpm deps:manifest:check` **fails** if the local pin is behind the latest upstream nightly, and the release workflow's `gate` job runs it. So **you cannot cut a release with a stale yt-dlp pin.** Since the pin is compiled into the Go binary, users only get the bump by installing a new app release.
   - Note this makes the app release cadence the update channel for yt-dlp — the opposite trade-off from a runtime self-update design.
   - The manifest generator deliberately avoids the GitHub API: *"不用 GitHub API：未认证时很快会被限流（403），改为读 releases/latest 的 302 跳转地址，稳定且无配额"* — it reads the `Location` header of `https://github.com/<repo>/releases/latest` and falls back to the API only if that fails. It also uses `curl` rather than Node `fetch` because *"Node 的 fetch 不读取 HTTP(S)_PROXY 环境变量"*.
2. **App self-update: YES, with a hand-rolled macOS path.**
   - Windows (NSIS) / Linux (AppImage, deb, rpm): `electron-updater`.
   - **macOS: custom, because the app is unsigned and Squirrel.Mac's signature check always fails.** Flow: download the arch-matched `.zip` → stream-hash **sha512** and compare to the manifest → `ditto -x -k` extract → `xattr -dr com.apple.quarantine` → `codesign --force --deep --sign -` (ad-hoc) → write a detached `/bin/bash` script that waits for the PID to exit, moves the old `.app` to a backup, `ditto`s the new one in, strips quarantine, deletes the backup, `open`s the app, and **rolls back on failure**. If the install dir isn't writable it re-runs the same script via `osascript ... with administrator privileges`.
   - Unsupported install kinds (portable `.exe`, `tar.gz`, running from the DMG) are detected and downgraded to "go to the releases page manually", with a reason string.
   - Robustness details: 60 s result cache to avoid re-hitting GitHub and re-nagging; single in-flight check promise so a dialog + About-page button don't double-request; `withTimeout(…, 20_000)`; a **GitHub API fallback** when `electron-updater` fails, with the comment that a failed check must not be reported as "already up to date" (the source of "there's a new version but it won't detect it"); friendly error mapping for ENOTFOUND/ECONNRESET, sha512 mismatch, 403 rate limit, 404, ENOSPC, EACCES; and a **dedicated `electron-updater` session partition** so the user's proxy setting applies (`autoUpdater` otherwise bypasses it).
   - `stripOwnQuarantine()` runs at startup to `xattr -dr com.apple.quarantine` its own bundle.

### D. Packaging / release
- **21 assets per release** (v1.3.4, 2026-10-14), verified via the releases API:
  - macOS: `DownVid-arm64-Mac-1.3.4.dmg` (128 MB) + `.zip` (127.7 MB); `DownVid-x64-Mac-1.3.4.dmg` (131.9 MB) + `.zip`
  - Windows: `DownVid-Setup-1.3.4.exe` (111.5 MB, NSIS), `DownVid-Portable-1.3.4.exe` (111.2 MB) — portable is given a **distinct artifact name on purpose**, because a name collision with the installer breaks `latest.yml` verification and auto-update
  - Linux: `AppImage` x86_64 (125.3 MB) + arm64 (126.1 MB), `deb` amd64 (100.2 MB), `rpm` (89.7 MB), `tar.gz` x64/arm64 (~119 MB)
  - Updater manifests: `latest.yml`, `latest-mac.yml`, `latest-linux.yml`, `latest-linux-arm64.yml`, plus `.blockmap` files
- **CI: 11 workflows** — `ci-linux.yml`, `ci-macos.yml`, `ci-windows.yml`, `codeql.yml`, `dependency-review.yml`, `release-audit.yml`, `release.yml` ("Build & Release"), `triage.yml`, `update-deps.yml`, + 2 Dependabot.
- `release.yml` is a 405-line, gated pipeline worth copying:
  1. `version` job: derives the version from the tag or a `workflow_dispatch` input, **fails if it disagrees with `package.json`**, resolves a `prerelease` flag (with a comment that GitHub's `/releases/latest` and `releases.atom` — which `electron-updater` uses — skip prereleases, so betas don't reach stable users).
  2. `gate` job (the release gate): `gofmt -l`, `go vet ./...`, `go test -shuffle=on -count=1 ./...`, `vue-tsc --noEmit`, contract lints (`lint:ipc`, `lint:icons`, `lint:website`, `deps:manifest:check`), unit tests, `test:updater`, `test:settings:migration`, `test:download:integration`, `test:parse:integration`, and a **cross-compile of the sidecar for all 5 targets** — "任一编不过，对应平台的用户就装不上".
  3. Five build jobs: mac arm64 + x64 (macos-latest), win (windows-latest), linux x64 (ubuntu-latest, matrix over appimage/deb/rpm/tar.gz), linux arm64 (ubuntu-24.04-arm).
  4. `release` job: **regenerates the `latest*.yml` manifests centrally** (because per-job ones overwrite each other), generates release notes with a "click to download" header, publishes with `softprops/action-gh-release@v3`, then **re-verifies the release after publishing** (`scripts/verify-release.mjs`) — added after a run was cancelled and left a tag + changelog + bumped version but **no GitHub Release**, so no existing user could ever receive the fix.

---

## 3. Bundling yt-dlp + FFmpeg into a self-contained desktop app

### 3.1 Standalone yt-dlp binaries (real, measured sizes)

Latest **stable** release at time of research: tag `2026.08.19`, published 2026-08-19. Sizes from the GitHub releases API:

| Asset | Size | Notes |
|---|---|---|
| `yt-dlp` | **2.93 MB** | Platform-independent **zipimport** binary. **Needs Python.** Recommended for Linux/BSD. |
| `yt-dlp.exe` | **17.01 MB** | Windows (Win8+) **standalone** x64. Recommended for Windows. |
| `yt-dlp_x86.exe` | 12.59 MB | Windows standalone x86 (32-bit) |
| `yt-dlp_arm64.exe` | 20.18 MB | Windows (Win10+) ARM64 |
| `yt-dlp_macos` | **35.43 MB** | Universal macOS (10.15+) standalone |
| `yt-dlp_linux` | **38.57 MB** | Linux (glibc 2.17+) standalone x86_64 |
| `yt-dlp_linux_aarch64` | 38.31 MB | Linux glibc 2.17+ aarch64 |
| `yt-dlp_musllinux` / `_aarch64` | 38.65 / 37.96 MB | musl 1.2+ (Alpine) |
| `yt-dlp_win.zip`, `yt-dlp_macos.zip`, `yt-dlp_linux.zip`, … | ~+0.1 MB over binary | "Unpackaged … executable (**no auto-update**)" |
| `yt-dlp.tar.gz` | 5.74 MB | Source tarball |
| `SHA2-256SUMS` / `SHA2-512SUMS` (+ `.sig`) | ~0 | GNU-style `hash  filename` sums, each with a **GPG signature**; public key at `github.com/yt-dlp/yt-dlp/blob/master/public.key` |
| `_update_spec` | ~0 | Channel/version lockfile consumed by the self-updater |

- **Latest nightly** (`yt-dlp/yt-dlp-nightly-builds`) is tag **`2026.10.10.233141`** — **newer than stable `2026.08.19`** — and has an identical asset set with near-identical sizes (`yt-dlp.exe` 17.02 MB, `yt-dlp_linux` 38.58 MB).
- **Upstream officially recommends nightly for regular users:** stable *"is often 'stale' and prone to external breakage (i.e. sites changing things on their end and breaking yt-dlp)"*, and nightly *"is the **recommended channel for regular users**"*. Any user hitting a bug on stable is told to move to nightly before filing a report.
- Sources: https://github.com/yt-dlp/yt-dlp#release-files , https://github.com/yt-dlp/yt-dlp#update-channels , and the releases API (`/repos/yt-dlp/yt-dlp/releases/latest`, `/repos/yt-dlp/yt-dlp-nightly-builds/releases/latest`).

### 3.2 Does a standalone yt-dlp.exe need Python at runtime?

**No.** The official README documents `yt-dlp.exe` as the "Windows (Win8+) **standalone** x64 binary", and `yt-dlp_macos`/`yt-dlp_linux` as "**standalone** executable". These are PyInstaller builds that embed the CPython interpreter (`update.py` inspects `sys.frozen` / `sys._MEIPASS` and handles both onefile and onedir PyInstaller layouts), so **no Python installation is required on the end user's machine**.

Contrast: the extension-less `yt-dlp` (2.93 MB) is explicitly documented as a "Platform-independent **zipimport** binary. **Needs Python**" — that one requires a Python 3.10+ runtime (README DEPENDENCIES: "Python versions 3.10+ (CPython) and 3.11+ (PyPy) are supported").

**Practical rule: ship `yt-dlp.exe` / `yt-dlp_macos` / `yt-dlp_linux[_aarch64]`, never the bare `yt-dlp`.**

### 3.3 ⚠️ Licensing surprise: the standalone binaries are GPLv3+

From the official README's Licensing section:

> "While yt-dlp is licensed under the Unlicense, many of the release files contain code from other projects with different licenses. Most notably, **the PyInstaller-bundled executables include GPLv3+ licensed code, and as such the combined work is licensed under GPLv3+.** The zipimport Unix executable (`yt-dlp`) and release tarball (`yt-dlp.tar.gz`) contain ISC licensed code from `meriyah` and MIT licensed code from `astring`."

So `yt-dlp.exe` / `_macos` / `_linux` are **GPLv3+ works**, while the git repo, PyPI sdist and wheel are Unlicense. Bundling the standalone binary into your installer means redistributing a GPLv3+ binary — relevant if your app is not GPL-compatible. (Downloading it at runtime into the user's own storage, as DownVid does, is a materially different posture.) Source: https://github.com/yt-dlp/yt-dlp#licensing

### 3.4 FFmpeg builds for bundling

**You need `ffmpeg` and `ffprobe`. `ffplay` is not needed by yt-dlp.**

| Source | Variants | Windows x64 archive | Licence |
|---|---|---|---|
| **gyan.dev** | `essentials`, `full`, `full-shared`; release + git master | essentials `.7z` **34 MB** / `.zip` **109 MB**; full `.7z` (size not published) | **all builds GPLv3** |
| **BtbN/FFmpeg-Builds** | `gpl`, `lgpl`, `nonfree`, `gpl-shared`, `lgpl-shared`, `nonfree-shared`; win64/win32/linux64/linuxarm64 | `win64-gpl.zip` **191.01 MB**, `win64-lgpl.zip` **169.56 MB**, `win64-gpl-shared.zip` **84.73 MB**; `linux64-gpl.tar.xz` 149.00 MB; `winarm64-gpl.zip` 134.91 MB | depends on variant |
| **yt-dlp/FFmpeg-Builds** | GPL (+ `gpl-shared`); win64/win32/winarm64/linux64/linuxarm64 | `win64-gpl.zip` **191.02 MB** | GPL; **no macOS builds** |
| **eugeneware/ffmpeg-static** | npm-oriented repackager, per-platform `ffmpeg-*`/`ffprobe-*` (+`.gz`) | `ffmpeg-win32-x64` **78.96 MB**, `.gz` 28.21 MB | inherits upstream (see below) |

**Measured individual binary sizes** (parsed the gyan.dev `ffmpeg-release-essentials.zip`, FFmpeg **9.0.2**, with `tar -tvf`):

```
ffmpeg.exe   105,423,872 B  = 100.54 MB
ffprobe.exe  105,221,120 B  = 100.34 MB
ffplay.exe   106,903,552 B  = 101.95 MB
```

**That is the single most important sizing fact here: gyan.dev's static Windows binaries are ~100 MB each, so bundling ffmpeg + ffprobe costs ~201 MB uncompressed** — far more than an entire Tauri app (typically 3–10 MB). The gyan.dev page also states all builds are "64-bit, **static** and licensed as **GPLv3**", and recommends git master over release builds.

For comparison, `eugeneware/ffmpeg-static` (FFmpeg 6.1.1) sizes, straight from its releases API — `ffmpeg-win32-x64` 78.96 MB, `ffprobe-win32-x64` 78.84 MB, `linux-x64` 76.13 MB, `linux-arm64` 48.77 MB, `darwin-x64` 75.21 MB, `darwin-arm64` 43.46 MB (ffprobe ≈ equal; `.gz` variants roughly 1/3 the size). Still ~158 MB for ffmpeg+ffprobe on Windows.

**essentials vs full — which to use?** For a downloader, **essentials is the right choice**, and it already contains everything yt-dlp needs. gyan.dev's own guidance: *"If you're downloading a package to support features in a program like Krita or Blender, the **release essentials** build is sufficient."* The essentials library list explicitly includes **`libx264`, `libx265`, `libmp3lame`, `libopus`, `libvpx`, `libvorbis`, `libass`**, etc. — so muxing, mp4 remuxing and mp3 extraction all work. `full` adds ~40 niche libraries (`libdav1d`, `libplacebo`, `libbluray`, `vulkan`, `whisper`, `frei0r`, `ladspa`, …) that a yt-dlp GUI does not need, at a larger size.

**BtbN variants** are documented in its README:
- `gpl` — "Includes all dependencies, even those that require full GPL instead of just LGPL."
- `lgpl` — "Lacking libraries that are GPL-only. Most prominently **libx264 and libx265**."
- `nonfree` — adds `fdk-aac` (not redistributable).
- `*-shared` — "comes with the libav* family of shared libs instead of pure static executables" (smaller archive, but you must ship the DLLs alongside and keep them together).
- Build targets: `win64`, `win32`, `linux64` (glibc ≥ 2.28, linux ≥ 4.18), `linuxarm64`. Windows builds target Win7+ with UCRT, minimum supported Win10 22H2. **No macOS builds** (yt-dlp/FFmpeg-Builds README: "There are currently no MacOS builds. If you know how to add them to the workflow, make a PR").
- Retention: "The last build of each month is kept for two years. The last 14 daily builds are kept. The special `latest` build floats and provides consistent URLs always pointing to the latest build." Builds run daily at 12:00 UTC.

**yt-dlp/FFmpeg-Builds** exists specifically for yt-dlp integration but currently applies **no patches** — its README says "currently our builds are equivalent to upstream ffmpeg", and yt-dlp's README confirms "currently our builds are equivalent to upstream ffmpeg". So there is no longer a yt-dlp-specific advantage over BtbN; prefer whichever licence/size fits.

**eugeneware/ffmpeg-static licensing caveat:** it is not a build system, it is a **repackager**. Its README documents that it pulls Windows x64 from **gyan.dev** (→ GPLv3), Windows x86 from sudo-nautilus/FFmpeg-Builds-Win32, Linux from johnvansickle.com, and macOS from evermeet.cx / osxexperts.net. So its Windows binaries inherit gyan.dev's **GPLv3** terms. It also pins **FFmpeg 6.1.1** ("The ffmpeg version currently used is `6.1.1`"), while current upstream is **9.0.2** — roughly two major versions stale (this is exactly what DownVid ships).

### 3.5 FFmpeg licensing: LGPL vs GPL when redistributing

From https://www.ffmpeg.org/legal.html:

- "FFmpeg is licensed under the **GNU Lesser General Public License (LGPL) version 2.1 or later**. However, FFmpeg incorporates several optional parts and optimizations that are covered by the **GNU General Public License (GPL) version 2 or later**. **If those parts get used the GPL applies to all of FFmpeg.**"
- "Note that FFmpeg is **not available under any other licensing terms**, especially not proprietary/commercial ones, not even in exchange for payment."

ffmpeg.org's LGPL compliance checklist (condensed) — the actionable items for a desktop app:
1. Compile **without `--enable-gpl` and without `--enable-nonfree`**.
2. **Use dynamic linking** (on Windows: link to DLLs).
3. **Distribute the FFmpeg source code**, modified or not, corresponding exactly to the binaries you ship; include a `changes.diff` if modified and a text file explaining your `configure` line.
4. **Host the FFmpeg source on the same webserver as the binary you distribute.**
5. Add an attribution + source-download link on **every page of your website that links to your app download**.
6. Mention "This software uses libraries from the FFmpeg project under the LGPLv2.1" in your program's **about box**.
7. Mention FFmpeg usage under LGPLv2.1 in your EULA; do not claim ownership of FFmpeg; **remove any prohibition of reverse engineering** from your EULA.
8. "Make sure your program is **not using any GPL libraries (notably libx264)**."

**Concrete implications for a yt-dlp GUI:**
- **GPL build (gyan.dev, BtbN `gpl`, yt-dlp/FFmpeg-Builds):** you are redistributing a GPLv2+/GPLv3 binary. That is fine if your app's licence is GPL-compatible; otherwise you take on GPL obligations for the combined distribution (source availability, no additional restrictions). **gyan.dev ships only GPLv3 builds**, so gyan.dev cannot be used for an LGPL-only posture.
- **LGPL build (BtbN `lgpl`, or build your own without `--enable-gpl`):** keeps FFmpeg separable, but **drops libx264 and libx265**. For a *downloader* this is usually acceptable — yt-dlp uses ffmpeg mainly for **stream copy / muxing / remuxing and audio extraction**, not for H.264 encoding. You would only miss libx264 if you re-encode video or convert audio to formats needing it (though `libmp3lame` is not GPL-only, so mp3 extraction still works). Note LGPL compliance also wants **dynamic linking**, which pushes you toward the `*-shared` variants plus their DLLs.
- **Runtime download (DownVid's approach):** the binary never enters your installer or your CI artifacts, which avoids redistributing FFmpeg entirely — the user downloads it from upstream themselves. This dramatically simplifies the licence question, at the cost of a first-run download.
- The patent section is deliberately non-committal; ffmpeg.org only warns that commercial products monetising patented codecs have attracted MPEG LA attention. Not a blocker for a downloader.
- Also note ffmpeg.org's trademark/typo rule: "Do not misspell FFmpeg (two capitals F and lowercase 'mpeg')."

### 3.6 Pointing yt-dlp at a bundled FFmpeg: `--ffmpeg-location`

Official help text (`yt-dlp --help`):
```
--ffmpeg-location PATH   Location of the ffmpeg binary; either the
                         path to the binary or its containing directory
```

Authoritative behaviour, read from `yt_dlp/postprocessor/ffmpeg.py` → `FFmpegPostProcessor._determine_executables()`:

```python
programs = ['ffmpeg', 'ffprobe']
location = self.get_param('ffmpeg_location', ...)
if location is None:
    return {p: p for p in programs}          # bare names -> resolved via PATH
if not os.path.exists(location):
    self.report_warning(f'ffmpeg-location {location} does not exist! '
                        'Continuing without ffmpeg', only_once=True)
    return {}                                 # NO ffmpeg at all
elif os.path.isdir(location):
    dirname, basename, filename = location, None, None
else:
    filename = os.path.basename(location)
    basename = next((p for p in programs if p in filename), 'ffmpeg')
    dirname = os.path.dirname(os.path.abspath(location))
paths = {p: os.path.join(dirname, p) for p in programs}
...
```

Consequences:
1. **Accepted forms:** a **directory** (yt-dlp then looks for `ffmpeg` *and* `ffprobe` inside it) **or** a **file path** (yt-dlp derives the sibling by replacing the `ffmpeg`/`ffprobe` substring in the filename).
2. **Recommendation: pass the DIRECTORY** containing both `ffmpeg` and `ffprobe`. That is what DownVid does (`getFfmpegLocation()` returns `path.dirname(...)`, with the comment "传目录而不是文件路径：yt-dlp 会在该目录里同时找 ffmpeg 与 ffprobe"). Videdown passes a file path, which is also legal but relies on the sibling-derivation path.
3. **If the path does not exist, yt-dlp does not fall back** — it warns and proceeds with **no ffmpeg at all**. Validate the path at startup.
4. **yt-dlp does NOT automatically look next to its own executable.** With no `--ffmpeg-location`, `_determine_executables` returns bare `'ffmpeg'`/`'ffprobe'`, which the OS resolves via `PATH`. I grepped the downloaded `yt_dlp/__init__.py`, `__main__.py`, `postprocessor/common.py` and `update.py` for `environ['PATH']` / `pathsep` / `sys.executable` prepending and **found no code that adds yt-dlp's own directory to `PATH`**. So "just drop ffmpeg next to yt-dlp" is **not** sufficient — pass `--ffmpeg-location` explicitly (or prepend the directory to the child process's `PATH` yourself).
5. There is a related known issue: "ffmpeg not selected as downloader when not in PATH" (yt-dlp issue #2191) — consistent with the above.

Also relevant: yt-dlp's README lists `ffmpeg` **and `ffprobe`** as "highly recommended" (not optional) because they are needed for merging separate video/audio streams and for post-processing. And: "**Important**: What you need is ffmpeg *binary*, **NOT** the Python package of the same name."

### 3.7 Updating yt-dlp programmatically

**What `-U` does and does not support** (README + `yt_dlp/update.py`):

| Install method | Self-update? |
|---|---|
| Release binaries (`yt-dlp.exe`, `_macos`, `_linux`, …) | **Yes** — "You can use `yt-dlp -U` to update if you are using the release binaries" |
| zipimport `yt-dlp` | Yes — `detect_variant() == 'zip'` is replaced **in place** |
| pip | No — "simply re-run the same command that was used to install the program" |
| Third-party package managers | No — "You installed yt-dlp from a manual build or with a package manager; Use that to update" |
| **Unpackaged `.zip` variants** | **No** — "Auto-update is not supported for unpackaged executables; Re-download the latest release" |
| Running from source | No — "You cannot update when running from source code; Use git to pull the latest changes" |
| Unofficial build | No — "You are using an unofficial build of yt-dlp; Build the executable again" |

So **prefer the non-`.zip` standalone assets** specifically because the `.zip` ones lose self-update.

**Channel semantics:** `-U`/`--update` only moves within the *current* channel. `--update-to CHANNEL` switches channels; `--update-to [CHANNEL@]TAG` upgrades/downgrades to a specific tag; `--update-to <owner>/<repo>` points at a different repository ("there is no verification done for binaries from different repositories"). Examples from the README: `yt-dlp --update-to nightly`, `yt-dlp --update-to stable@2023.07.06`, `yt-dlp --update-to master`. A version older than 90 days prints an update nag; suppress with `--no-update`.

**Will `-U` work for a bundled binary? Effectively no, if the install dir is read-only.** `Updater.update()` explicitly gates on write access:

```python
directory = os.path.dirname(self.filename)
if not os.access(self.filename, os.W_OK):
    return self._report_permission_error(self.filename)
elif not os.access(directory, os.W_OK):
    return self._report_permission_error(directory)
new_filename, old_filename = f'{self.filename}.new', f'{self.filename}.old'
```

It writes a `.new` sibling and swaps via `.old` — so it needs write access to **both the binary and its containing directory**. Inside `C:\Program Files\...`, a macOS `.app` bundle, an AppImage mount, or a snap/flatpak sandbox this fails with a permission error. Practical options:
1. **Recommended:** don't bundle — install yt-dlp into a writable per-user directory (DownVid uses `app.getPath('userData')/runtime/bin`; for Tauri use `app_data_dir()`), then you can either run `-U` there or manage the download yourself.
2. **Or** implement the update yourself: query the release API, download the platform asset, verify against `SHA2-256SUMS`, write it atomically, set the executable bit, and relaunch/renumber. This gives you control over pinning and hashing (DownVid's model).
3. `-U` also cannot be pointed at an arbitrary install layout safely; `--update-to <owner>/<repo>` explicitly performs **no verification**, so don't rely on it for supply-chain integrity.

**Querying the latest version from GitHub:**
- Endpoint: **`GET https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest`** → read **`tag_name`** (e.g. `2026.08.19`). This is exactly what yt-dlp's own updater does (`Updater._call_api` builds `f'{API_BASE_URL}/{repo}/releases/{tag}'` with `tag='latest'`, and `_get_version_info` reads `api_info['tag_name']`), sending headers:
  `Accept: application/vnd.github+json`, `User-Agent: yt-dlp`, `X-GitHub-Api-Version: 2026-03-10`.
- Nightly: `GET https://api.github.com/repos/yt-dlp/yt-dlp-nightly-builds/releases/latest` → `2026.10.10.233141`. Master: `.../yt-dlp-master-builds/releases/latest`.
- For a specific tag: `/releases/tags/<tag>`.
- **Rate limit (measured, not assumed):** an unauthenticated request returned `X-RateLimit-Limit: 60`, `X-RateLimit-Resource: core`, `X-RateLimit-Remaining: 38` — i.e. **60 requests/hour per IP**; authenticated requests get 5,000/hour. Docs: https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api
- **Caching:** the same response carries a weak `ETag` (`W/"fb40…"`) and `Last-Modified`. Send `If-None-Match` and treat `304` as "unchanged", and cache the result client-side for at least hours. **[UNVERIFIED]** I could not load the GitHub docs confirmation that a `304` is exempt from the rate limit (the docs pages are JS-rendered and truncated in fetch); treat conditional requests as a large reduction in usage rather than a guaranteed exemption.
- **Cheaper alternative with no quota at all (DownVid's trick):** `curl -sS -o /dev/null -w '%{redirect_url}' https://github.com/yt-dlp/yt-dlp/releases/latest` returns the `302` `Location`, whose `/tag/<TAG>` segment is the latest version. No API, no rate limit. DownVid uses this as the primary method and falls back to the API only if a proxy follows redirects automatically. In Node/Electron the equivalent is an HTTP client with redirects disabled; in Rust, `reqwest` with `redirect::Policy::none()`.
- **Version formats / comparison:** stable tags are **date-based `YYYY.MM.DD`** (`2026.08.19`); nightly tags are **`YYYY.MM.DD.HHMMSS`** (`2026.10.10.233141`); master builds use a similar timestamped form. Compare segment-wise as integers, and **only compare within a channel** — a nightly tag is legitimately "greater" than a stable tag of an earlier date, which is expected, not a bug (that is exactly why nightly `2026.10.10.233141` is ahead of stable `2026.08.19`). `yt-dlp --version` prints the local version for comparison.
- `_update_spec` (present in both stable and nightly releases) is the updater's lockfile; its content maps channels to repositories and pins binary-name/version rules, e.g.:
  ```
  lockV2 yt-dlp/yt-dlp 2025.10.14 zip Python 3\.9
  lockV2 yt-dlp/yt-dlp-nightly-builds 2025.10.14.232845 zip Python 3\.9
  lockV2 yt-dlp/yt-dlp-master-builds 2025.10.14.232330 zip Python 3\.9
  ```

### 3.8 ⚠️ The dependency everyone forgets: a JavaScript runtime (and yt-dlp-ejs)

yt-dlp's README states: *"While all the other dependencies are optional, **`ffmpeg`, `ffprobe`, `yt-dlp-ejs` and a supported JavaScript runtime/engine are highly recommended**"*.

- `yt-dlp-ejs` (https://github.com/yt-dlp/ejs) is *"Required for full YouTube support"* (Unlicense, bundling MIT/ISC components).
- A JS runtime is needed to run it. The `--js-runtimes` help text: *"Supported runtimes are (in order of priority, from highest to lowest): **deno, node, quickjs, bun. Only 'deno' is enabled by default.** The highest priority runtime that is both enabled and available will be used. In order to use a lower priority runtime when 'deno' is available, `--no-js-runtimes` needs to be passed before enabling other runtimes."*
- Without a JS runtime, yt-dlp drops YouTube player clients: *"If no JavaScript runtime/engine is available, then `web` and `tv_samsung` are omitted."*
- `--remote-components ejs:npm|ejs:github` exists to let yt-dlp fetch the JS components remotely — *"currently not needed if you are using an official executable"* — and **no remote components are allowed by default**.

**Implication for a "no install required" Tauri app:** you need **three** bundled/obtained things for full YouTube support, not two — yt-dlp, ffmpeg(+ffprobe), **and a JS runtime**. Deno is upstream's recommended default. Measured Deno v2.9.7 release sizes: `deno-x86_64-pc-windows-msvc.zip` **40.7 MB**, `deno-x86_64-unknown-linux-gnu.zip` 39.7 MB, `deno-x86_64-apple-darwin.zip` 40.3 MB, `deno-aarch64-apple-darwin.zip` 36.7 MB — and a runtime-only **`denort`** variant at ~31 MB zipped (`denort-x86_64-pc-windows-msvc.zip` 31.2 MB), which is the leaner choice if you only need to execute the EJS scripts.

Both reference projects handle this by **asking the user to install Node.js** (videdown probes `C:\Program Files\nodejs\node.exe` etc.; DownVid requires Node 22+ or Deno 2.3+ and prompts with a dialog). Neither bundles it. Also note a subtlety both get slightly wrong: neither passes `--no-js-runtimes` first, so per the documented priority order, if the user happens to have Deno installed, Deno wins over the explicit Node path they passed.

### 3.9 Tauri-specific sidecar mechanics

From https://v2.tauri.app/develop/sidecar/ (docs source: https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/develop/sidecar.mdx):

**Declaring the binaries**
```json
{ "bundle": { "externalBin": ["binaries/yt-dlp", "binaries/ffmpeg", "binaries/ffprobe"] } }
```
Paths are relative to `src-tauri/tauri.conf.json` (so `binaries/yt-dlp` = `<project>/src-tauri/binaries/yt-dlp`).

**Target-triple suffix is mandatory at build time.** "To make the external binary work on each supported architecture, a binary with the same name and a `-$TARGET_TRIPLE` suffix must exist on the specified path." E.g. `binaries/yt-dlp-x86_64-pc-windows-msvc.exe`, `binaries/yt-dlp-aarch64-apple-darwin`. Get your triple with `rustc --print host-tuple` (Rust ≥ 1.84), or `rustc -Vv | Select-String "host:"` on Windows PowerShell. The docs give a Node script that renames `sidecar` → `sidecar-${targetTriple}` — and warn it "will not work if you compile for a different architecture than the one it's running on", so your CI needs a per-target rename step.

> **[UNVERIFIED]** Neither the v1 nor the v2 docs state explicitly whether Tauri *strips* the `-$TARGET_TRIPLE` suffix from the filename inside the installed bundle. The docs only establish (a) the suffix is required on disk at build time, and (b) at runtime you reference the **base name**. Do not hardcode a filesystem path to the bundled binary without checking the produced bundle layout for your target.

**Spawning and streaming output line-by-line (Rust)**
```rust
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandEvent;
use tauri::Emitter;

let sidecar_command = app.shell().sidecar("yt-dlp").unwrap();
let (mut rx, mut child) = sidecar_command.spawn().expect("Failed to spawn sidecar");

tauri::async_runtime::spawn(async move {
    while let Some(event) = rx.recv().await {
        if let CommandEvent::Stdout(line_bytes) = event {
            let line = String::from_utf8_lossy(&line_bytes);
            app.emit("ytdlp-progress", Some(line.to_string())).unwrap();
            // child.write(b"...\n").unwrap();
        }
    }
});
```
This `CommandEvent::Stdout` stream is exactly what you pair with `--progress-template` + `--newline`: yt-dlp emits one `[dvp]…` line per update and Tauri hands you one event per line. For JS, `Command.sidecar('binaries/yt-dlp', args)` with `.spawn()` gives a `Child` whose `stdout.on('data')` you can line-split.

**Permission model — the biggest practical gotcha.** The shell plugin's **default permission set only allows `open`** (`shell:allow-open`); execute/spawn are denied unless you explicitly grant them. Available identifiers (from the plugin's generated permissions reference): `shell:allow-execute`, `shell:allow-spawn`, `shell:allow-kill`, `shell:allow-stdin-write` (plus `deny-*` counterparts).

You must add a scoped permission in `src-tauri/capabilities/default.json`:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main"],
  "permissions": [
    "core:default",
    {
      "identifier": "shell:allow-spawn",
      "allow": [
        { "name": "binaries/yt-dlp", "sidecar": true, "args": true }
      ]
    }
  ]
}
```
- Use `shell:allow-execute` if you start the sidecar with `command.execute()`, or `shell:allow-spawn` (or the `.spawn()` variant) for long-running streaming — the docs explicitly note: *"The `shell:allow-execute` identifier is used because the sidecar's child process will be started using the `command.execute()` method. To run it with `command.spawn()`, you need to change the identifier to `shell:allow-spawn`."* For a downloader you want **spawn**, not execute, since `execute()` buffers.
- **Arguments are validated positionally.** *"A value of `true` will allow any arguments to be passed to the command. `false` will disable all arguments. If neither `true` or `false` is set, you define the arguments in the exact order in which you'd call them. Static arguments are defined as-is, while dynamic arguments can be defined using a regular expression."* A dynamic arg looks like `{ "validator": "\\S+" }`. The JS call must then pass an args array that "matches EXACTLY what is specified in `capabilities/default.json`".
  - For a yt-dlp GUI this means either `"args": true` (simplest, but wide open — any arg including `--exec`), or an ordered allow-list with validators. **Note the risk:** yt-dlp has `--exec` (arbitrary command execution) and `--config-locations`; if you allow arbitrary args you have effectively granted command execution to your renderer. Prefer a strict ordered list with URL/path regex validators.
- `sidecar()` naming: *"The `sidecar()` function expects just the filename, NOT the whole path configured in the `externalBin` array."* With `externalBin: ["binaries/app", "my-sidecar", "../scripts/sidecar"]`, you call `app.shell().sidecar("app")` / `"my-sidecar"` / `"sidecar"`. From JS, `Command.sidecar()` takes the string as it appears in `externalBin`.

**Other Tauri considerations**
- **Locating the sidecar to pass `--ffmpeg-location` at it:** rather than guessing the bundle layout, resolve the directory at runtime (`app.path().resource_dir()`, or resolve the current executable's directory) and pass **that directory** to `--ffmpeg-location` so yt-dlp finds both `ffmpeg` and `ffprobe`. This is the single most important integration detail, given §3.6 point 4 (yt-dlp will not look next to itself).
- **Executable bits:** ensure `chmod +x` on the sidecars; on macOS/Linux the files must be executable inside the bundle.
- **Code signing / notarization:** shipping extra executables inside a `.app` means each one must be signed with your Developer ID and covered by the hardened runtime, or notarization/Gatekeeper will block them. DownVid works around the unsigned case with `xattr -dr com.apple.quarantine` + ad-hoc `codesign --force --deep --sign -`, plus a startup `stripOwnQuarantine()`. Expect to do the equivalent (or sign properly).
- **On Windows,** killing a yt-dlp download requires killing the process **tree**: DownVid uses `taskkill /PID <pid> /T /F` with the comment that killing the parent first loses the PyInstaller child. Tauri's `shell:allow-kill` covers the direct child only — budget for tree-kill on Windows.
- **Tauri's updater plugin** (`tauri-plugin-updater`) handles app self-update; note DownVid's hard-won lessons: cache check results (~60 s), coalesce concurrent checks, never report "up to date" on a failed check, and route updater traffic through the configured proxy (electron-updater uses a separate session and silently bypasses it).

### 3.10 Synthesis: what the evidence says you should do

**Size reality check.** For a Tauri app (typically 3–10 MB binary), naively bundling looks like:
- yt-dlp.exe 17.0 MB + gyan essentials ffmpeg 100.5 MB + ffprobe 100.3 MB = **~218 MB** (+ ~40 MB if you also bundle Deno) vs. BtbN `*-shared` (84.7 MB zipped for the whole toolset) or eugeneware/ffmpeg-static (~158 MB for ffmpeg+ffprobe on Windows).
- Bundling dominates the app size by 20–60×. The two reference projects — after one of them had a year of production iteration and a rewritten architecture — **both deliberately stopped bundling** and moved to detect-then-download into user storage.

**Recommended architecture (mirrors DownVid, adapted to Tauri):**
1. **Install location:** per-user writable dir (`app_data_dir()/runtime/bin`), never inside the read-only app bundle or Program Files. This is what makes updating possible at all.
2. **Resolve order:** manual override → managed dir → system PATH → common install dirs. Cache the detection result briefly (~60 s) and re-probe after an install.
3. **Pin, verify, and record versions:** bake a manifest of `{platform, asset name, SHA-256, required version}`; verify SHA-256 before use (`SHA2-256SUMS` for yt-dlp; you must compute your own for FFmpeg and for eugeneware/ffmpeg-static); gunzip if needed; `chmod +x`.
4. **Track the nightly channel**, not stable. Upstream recommends it for regular users, and DownVid's code documents that stable `2026.08.19` actually fails on YouTube while nightly `2026.10.10.233141` works.
5. **Enforce a minimum yt-dlp version** equal to your pinned version, and use a **full date comparison**, not a year comparison — the code comment on this is specific and correct.
6. **Force pin freshness in CI:** a `--check` mode that fails if your pin is behind upstream, wired into the release gate, so you cannot ship a stale yt-dlp.
7. **Parse progress with `--progress-template`**, never regex on human-readable text. Custom prefixes + `startsWith` dispatch, `--newline --progress`, and `--print after_move:...%(filepath)s` for the output path.
8. **Pass `--ffmpeg-location <directory>`** and validate it exists.
9. **Cap concurrency at ~3** with a single queue gate; have "merging" occupy a slot too.
10. **Add `--http-chunk-size`** — DownVid measured >30× on a throttled CDN link, and it works on single-file mp4 direct links where `--concurrent-fragments` does nothing.
11. **Pass `--ignore-config`** so a user's global yt-dlp config file cannot silently change your app's behavior.
12. **Budget for a JS runtime** (Deno recommended, `denort` ~31 MB if size matters) or accept "please install Node" like both reference projects do.
13. **Licensing:** if you bundle `yt-dlp.exe` you ship a **GPLv3+** binary; if you bundle FFmpeg you ship **GPLv3** (gyan.dev) or **LGPL** (BtbN `lgpl`, no libx264/x265) with source-offer and attribution obligations. Runtime download side-steps the redistribution question entirely.
14. **Windows process-tree kill** for pause/cancel.

---

## 4. Explicitly unverified / caveats

- Which electron-builder config videdown actually uses (`electron-builder.json5` vs the `build` block in `package.json`) — both exist and disagree on targets. I did not verify resolution precedence.
- Whether Tauri strips the `-$TARGET_TRIPLE` suffix from the filename inside the installed bundle. The docs only state the build-time naming requirement and the runtime base-name reference.
- Whether a GitHub API `304` conditional response is exempt from the 60/hour rate limit. The `ETag`/`Last-Modified` headers are present and confirmed; the exemption claim is not confirmed from official docs (their pages did not render fully on fetch).
- Both repos' Chinese-language in-code comments were read in the original; translations in this report are mine. Some files echoed with mojibake when printed to the console, so I relied on the `read`/`grep` tools for Chinese text.
- I did not run either application, so all feature claims are from source code, READMEs, and release metadata — not from observed runtime behaviour.
- `videdown`'s behaviour on macOS/Linux is inferred from code (hardcoded `yt-dlp.exe` fallback) and from the absence of any mac/Linux release artifact; I did not test those platforms.
- I did not verify DownVid's `--http-chunk-size` speed claim (0.16 MB/s → 5 MB/s) myself; it is the project's own in-code measurement.
