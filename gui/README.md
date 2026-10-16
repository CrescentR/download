# Yuki · 雪迹

Yuki 是一个基于 **yt-dlp** 与 **FFmpeg** 的桌面视频下载器。它不重写下载核心，而是给 `yt_dlp` 套上一层的原生界面：**先解析、再选择、后下载**。

界面使用 Tauri 2 + Tailwind CSS 4 + 原生 ES 模块前端（无 JavaScript UI 框架），下载引擎用 Rust 编写。

---

## 功能

| 能力 | 说明 |
| --- | --- |
| **下载前解析** | 粘贴链接后先向 yt-dlp 取回真实标题、封面、时长、作者、播放量，以及**可选的清晰度列表**（含格式 ID、编码、体积），确认无误再下载。 |
| **真实格式选择** | 解析结果里的每个格式都可以直接选，选中后以该 `format_id` 下载，不再是「1080p / 720p」这种没作用的标签。 |
| **批量下载** | 输入框支持多行，一行一个链接；也可在解析后直接下载整个播放列表，或用范围（如 `1-10,15`）限定。 |
| **Cookie 登录** | 内置「从浏览器读取 Cookie」（Chrome / Edge / Firefox / Brave / Opera / Vivaldi / Safari …）与 cookies.txt 文件选择。YouTube 提示「Sign in to confirm you're not a bot」时用这个解决。 |
| **代理与限速** | 支持 HTTP / SOCKS5 代理、限速、重试次数、分片并发数。 |
| **任务详情与日志** | 点任意任务查看实时日志（按序号增量追加）、完整命令行（密钥自动打码）、输出路径，并可一键在资源管理器中定位文件。 |
| **可读的错误提示** | 失败时不再只丢一句 toast，而是识别常见原因（未登录、缺少 JS 运行时、缺少 FFmpeg、403、私享视频、超时…）并给出对应操作。 |
| **队列与并发** | 可配置同时下载数，超出的任务自动排队；取消会连同 ffmpeg 子进程一起结束。 |
| **主题与语言** | 深色 / 浅色主题，中文 / English 界面，均为即时切换并持久化。 |
| **依赖自检** | 检测 FFmpeg 与 JS 运行时（Node / Deno），缺失时给出指引或调用系统包管理器安装。 |

---

## 运行环境

- **Node.js** 24+（构建前端；同时作为 yt-dlp 的 JS 运行时，用于 YouTube 签名解析）
- **Rust** 工具链（构建 Tauri 后端）
- **Python** 3.10+（开发和源码模式需要；用户安装后可在应用内下载独立 yt-dlp，无需 Python）
- **FFmpeg**（合并音视频、转码、嵌入封面与字幕；强烈建议安装）

### yt-dlp 是怎么被找到的

本仓库的 `yt_dlp` **并没有 pip 安装**，所以 `python -m yt_dlp` 只是「碰巧能用」：Python 会把**当前工作目录**放进 `sys.path` 最前面，而那里正好有这个包。一旦工作目录变了，就会变成 `No module named yt_dlp`。

因此启动器按下面的顺序解析，并且**显式设置 `PYTHONPATH`**，不再依赖进程的工作目录：

1. 环境变量 `YUKI_YTDLP_BIN`（打包成 sidecar 时使用）
2. 用户目录中由 Yuki 下载并校验的独立 yt-dlp（优先于源码和 PATH，应用内更新立即生效）
3. 仓库源码模块：会**逐个探测**候选解释器（`PYTHON` 环境变量 → `python` → `python3` → `py`），只有真的能 `import yt_dlp` 的才会被选中，然后以 `python -m yt_dlp` 运行，并把仓库根目录写进子进程的 `PYTHONPATH`
4. `PATH` 中的 `yt-dlp` 可执行文件

如果都找不到，界面会在侧栏直接显示原因和修复建议，而不是等你点了下载才报一句看不懂的错。

---

## 开发

```powershell
cd gui
npm install

# 桌面应用（推荐，功能完整）
npm run dev

# 仅前端预览（浏览器打开 http://127.0.0.1:1420，使用示例数据，不启动真实下载）
npm run dev:frontend
```

检查与测试：

```powershell
cd gui/src-tauri
cargo check
cargo test --lib          # 下载引擎的参数构造与进度解析测试
```

## 打包

```powershell
cd gui
npm run build:frontend    # 只构建前端
npm run build             # 构建完整安装包
```

Windows 产物位于：

```text
gui/src-tauri/target/release/yuki.exe
gui/src-tauri/target/release/bundle/msi/
gui/src-tauri/target/release/bundle/nsis/
```

## 图标

图标由脚本生成（不是二进制黑盒），产物同时写入 Tauri 图标目录与 `public/`：

```powershell
pwsh -File devscripts/make-icons.ps1
```

## UI 校验

`devscripts/inspect-ui.mjs` 通过 Chrome DevTools Protocol 打开界面、执行交互脚本、输出元素几何信息与控制台报错，并截图。用于在改动界面后确认没有回归：

```powershell
# 用法: node devscripts/inspect-ui.mjs <url> <out.png> [宽] [高] [主题] [脚本文件]
node devscripts/inspect-ui.mjs http://127.0.0.1:1420/ shot.png 1280 820 dark devscripts/view-analyze.js
```

---

## 目录结构

```text
gui/
  index.html                  入口（含首屏主题预设，避免闪白）
  src/
    main.js                   启动、状态仓库、事件订阅、轮询兜底
    styles.css                设计系统（深色优先 + 浅色主题变量）
    core/
      api.js                  Tauri 桥接；浏览器预览的 stub 数据
      i18n.js                 中英文文案
      util.js                 格式化、URL 清理、DOM 与存储辅助
    ui/
      icons.js                内联 SVG 图标 + 品牌标识
      shell.js                外壳：侧栏、顶栏、状态栏；按需 patch
      preview.js              解析结果面板（格式选择 / 播放列表）
      tasklist.js             任务卡片（DOM 复用，避免闪烁）
      drawer.js               任务详情抽屉（日志增量追加）
      settings.js             设置弹窗（草稿式编辑）
      toasts.js               轻提示
      actions.js              全部用户动作（事件委托）
  src-tauri/
    src/
      lib.rs                  命令注册与下载队列编排
      download.rs             参数构造、进程监管、进度解析、取消、单元测试
      probe.rs                链接解析（--dump-single-json）
      deps.rs                 FFmpeg 检测与安装引导
      settings.rs             设置与历史持久化
      runtime.rs              定位 yt-dlp 与 JS 运行时
    tauri.conf.json           窗口与打包配置
```

---

## 设计要点

### 进度来自结构化模板，而不是正则抓屏

旧实现用正则去解析 yt-dlp 的人类可读输出。那个正则的可选分组没有锚点，实测会把 `size`、`speed` 解析为空，还把 `ETA` 错配成 `1.52MiB/s`。

现在下载时固定传入：

```text
--progress-template download:YUKIPROG|%(progress._percent_str)s|%(progress.downloaded_bytes)s|...
--print before_dl:YUKIMETA|title|%(title)s
--print after_move:YUKIMETA|file|%(filepath)s
```

界面只读这些机器可读字段，因此百分比、体积、速度、ETA 都是精确值。

### 界面不再整页重绘

旧实现每收到一行进度就 `innerHTML` 重建整个文档，并重新解析整套 lucide 图标；一个几十 MB 的文件会产生数百次全量重绘，界面直接卡死。

现在：

- 外壳只构建一次，其余按需 patch；
- 任务卡片按 task id 复用 DOM，缩略图 `src` 不变就不动，因此不闪烁、不丢滚动位置；
- 进度事件按 120ms 合并后再通知界面；
- 日志按序号增量追加；
- 图标是内联 SVG，移除了 `lucide` 依赖。

### 子进程输出走文件，不走管道

实测发现：**同一命令行在 shell 里能跑通，但由本程序用 `Stdio::piped()` 建管道时，yt-dlp 会以 `OSError: [Errno 22] Invalid argument` 中止**（退出码 120）。文件句柄则稳定可用。

因此每个任务的 stdout/stderr 被重定向到临时文件，再由一个 tail 线程按字节偏移跟进：

```text
%TEMP%\yuki-log-<task>.txt     子进程输出（读完后删除）
%TEMP%\yuki-meta-<task>.txt    标题 / 输出路径 / 时长等元数据
```

元数据之所以也走文件，是因为**冻结版 `yt-dlp.exe`（PyInstaller）无法通过 `PYTHONIOENCODING` 改变 stdout 编码**，仍使用系统 ANSI 代码页（中文 Windows 上是 cp936）；标题里出现该代码页无法表示的字符时，写 stdout 会让整个下载失败——而此时文件其实已经下完了。

另外，默认文件名模板从 `%(title).180B` 改为 `%(title).150s`：`.180B` 按**字节**截断，会把一个多字节汉字从中间劈开，产生非法字节序列，同样触发 `[Errno 22]`；`.150s` 按**字符**截断。

### 取消会结束整棵进程树

yt-dlp 会为合并调用 ffmpeg。只杀直接子进程会留下孤立的 ffmpeg 占用输出文件，因此 Windows 使用 `taskkill /T /F`，其他平台向进程组发送信号。

---

## 依赖与更新

安装包只带 Yuki 本体。yt-dlp 与 FFmpeg 由应用自己下载到用户目录，不需要管理员权限，也不改动系统环境：

```text
%APPDATA%\dev.yuki.downloader\bin\yt-dlp.exe
%APPDATA%\dev.yuki.downloader\bin\ffmpeg\{ffmpeg.exe, ffprobe.exe, *.dll}
```

- **yt-dlp**：下载官方 standalone 构建。它们是 PyInstaller 打包、自带 CPython 的版本，**用户无需安装 Python**。刻意不使用无扩展名的 `yt-dlp` 资产，那是 zipimport 构建，依赖系统 Python。
- **FFmpeg**：下载 BtbN 的 **LGPL shared** 构建，保留构建目录中的 `bin`、`lib` 与许可证文件。选 LGPL 而非 GPL，是因为下载器只需要流拷贝、封装与音频提取，用不到 GPL-only 的 x264/x265，同时保留下载构建随附的许可证与动态库。运行时下载并不自动豁免许可证义务。通过 `--ffmpeg-location <目录>` 指给 yt-dlp；该路径必须真实存在，否则 yt-dlp 会警告并**完全禁用** ffmpeg。
- **完整性**：yt-dlp 必须通过官方 `SHA2-256SUMS` 校验；FFmpeg 必须通过 GitHub asset SHA-256 digest 或发布校验清单。校验失败不安装。两者先试运行再替换，FFmpeg 整个目录一起切换，失败时恢复旧目录。
- **代理**：先读设置中的代理，其次读 `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` 等环境变量。没有直连 GitHub 的机器完全依赖这一点——否则只会得到一个语焉不详的连接超时。
- **版本检索**：启动后后台查询，设置 → 运行环境也可分别手动检查；不自动安装。yt-dlp 按数字版本比较；FFmpeg 按发布时间和资产摘要识别滚动构建，避免 `latest` 标签恒定造成漏报。检查失败会显示错误，不冒充「已是最新」。
- **平台**：Windows x64/ARM64、Linux x64/ARM64 使用对应的 BtbN LGPL shared 资产。macOS 的 yt-dlp 可托管更新；FFmpeg 当前通过现有安装/Homebrew 管理，BtbN 不提供 macOS 构建，界面会明确报告无匹配资产。
- **韧性**：断点续传（HTTP Range）+ 4 次重试与指数退避，针对 `os error 10060` 这类中途停滞。

### 为什么不用 yt-dlp 自带的 `-U`

它有两条硬限制：以源码方式运行时直接拒绝（`You cannot update when running from source code`）；并且需要同时拥有二进制**和其目录**的写权限，无法更新位于只读目录中的副本。自行下载并原子替换是唯一在各类安装形态下都可行的方案。

### 命令行自检

```powershell
yuki --check-updates        # 查询 GitHub 上最新版本（走代理）
yuki --install-ytdlp        # 下载、校验并安装 yt-dlp
yuki --install-ffmpeg       # 下载并安装 FFmpeg
yuki --diagnose <url>       # 用真实参数跑一次下载，打印完整命令行
```

`YUKI_DIAGNOSE_PIPED=1` 让 `--diagnose` 改用管道 stdio，`YUKI_DEBUG_ARGS=--verbose` 可附加任意 yt-dlp 参数，两者用于排查启动器问题。


## 安装包、CI 与开源准备

本仓库既有许可是 **Unlicense**，Yuki 沿用它，不替换上游 `LICENSE`。
安装包只包含应用，不预置 yt-dlp/FFmpeg。第一次打开时会检测可用运行时；
缺失或有更新时，用户在「设置 → 运行环境」点击安装/更新。首次安装依赖需要联网。
standalone yt-dlp 是 GPLv3+；FFmpeg 的许可随构建配置而变，详见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)，该说明与 LICENSE 一起进入安装包。

- `.github/workflows/yuki-ci.yml`：Windows / Linux / macOS 前端构建与 Rust 测试。
- `.github/workflows/yuki-release.yml`：Windows NSIS、Linux AppImage/deb、macOS Intel/Apple Silicon DMG。
  手动运行或推送 `yuki-v<版本>` 标签时构建，将产物上传到 Actions artifacts，**不会自动公开发布 Release**。
- `node scripts/check-release.mjs` 校验 npm、Cargo、Tauri、标签版本和打包许可证资源。
- 本地 Windows 安装包：`npm run build -- --bundles nsis`。
- 构建产物默认未签名。正式面向公众发布前需要配置自己的代码签名与 macOS notarization；本次没有替你创建证书或发布远程版本。

UI 回归：启动前端后，在仓库根目录执行：

```powershell
node devscripts/inspect-ui.mjs http://127.0.0.1:1420/ gui/qa-updates.png 1180 800 dark devscripts/verify-settings.js
node devscripts/inspect-ui.mjs http://127.0.0.1:1420/ gui/qa-compact.png 940 620 light devscripts/verify-settings.js
```

脚本验证五个设置页尺寸一致、选择路径立即显示、开关反馈、音频选项与两个依赖的更新按钮。
原生文件/目录选择由主窗口 capability 中的 `dialog:allow-open` 授权，事件订阅由 `core:default` 授权。

可通过 `YUKI_CONFIG_DIR` 为命令行自检指定隔离目录，避免测试更新覆盖日常使用的依赖。
只影响 headless 自检命令，GUI 仍使用系统应用配置目录。


## 0.3 桌面界面

- Matte Graphite / Warm Stone 两套不透明材质，36px 自绘标题栏、208px 侧栏、32px 状态栏。
- Tailwind 4 通过 Vite 插件在构建时生成样式，不使用 CDN。Inter / Noto Sans SC / JetBrains Mono 字体随应用分发。
- 链接平台识别、4K UHD / 1080p60 / Audio FLAC 预设、解析模态窗及字幕/封面选项。
- 任务按状态筛选，保留实际文件定位与设置/依赖更新功能。
- 暂停会结束当前下载进程并保留临时文件；继续用同一任务的原参数重新启动，向 yt-dlp 显式传入 `--continue`。
- 队列根据实际任务自动显示列表或空态；浏览器示例明确标为「界面预览」。
- 传输折线来自任务速度采样，分片线程数显示配置上限，不虚构系统线程数。

桌面界面检查（启动 `npm run dev:frontend` 后）：

```powershell
node devscripts/inspect-ui.mjs http://127.0.0.1:1420/ gui/qa-native.png 1180 800 dark devscripts/verify-native-ui.js
```

窗口控制通过 Tauri 原生 API，保留按拖动区域移动窗口及双击最大化。
暂停状态保存在当前应用会话中；此版本没有新增跨重启的任务恢复。
