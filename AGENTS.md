# AGENTS.md

## 项目简介

`dsh-launcher` 是 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
(`@deepseek-ai/dsh`) 网页版的**非官方 Windows 一键启动器**:双击 exe → 静默执行
`npx @deepseek-ai/dsh web` → 打开一个干净独立的 Chrome 窗口进入 DSH 页面 → 关闭该窗口即停止
服务并退出。仓库:<https://github.com/zhubingzuo/dsh-oneclick-launcher>(MIT)

## 技术栈

- Rust 2021,**零第三方依赖**:只用 `std` + 手写 Win32 FFI(kernel32 的 Job Object/命名互斥量、
  user32 的 `MessageBoxW`)
- GUI 子系统(`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`,release 下
  全程无控制台窗口)
- `build.rs` 调用 Windows SDK 的 `rc.exe` 嵌入 `assets/icon.ico`;找不到 `rc.exe` 也能构建,只是没图标
- release profile:`opt-level = "s"` + `lto = true` + `strip = true`
- 平台:仅 Windows / x86_64(`#[repr(C)]` 结构体布局按 winnt.h 写死)

## 主要模块

| 路径 | 用途 |
| --- | --- |
| `src/main.rs` | 全部程序逻辑(单文件):配置、单实例、Job Object、启动 dsh、就绪等待、开浏览器、关窗清理 |
| `build.rs` | 零依赖嵌图标(调用 `rc.exe`),失败静默降级 |
| `scripts/gen_icon.ps1` | 由 DSH 官方 `favicon.svg` 重新生成 `assets/icon.ico`(16–256 共 9 个尺寸) |
| `scripts/regression.ps1` | A/B/C/D/E 五套端到端回归(32 项判据),借助 `DSH_*` 开关,可与正在运行的实例并存 |
| `scripts/test-stub-server.ps1` | 场景 C/D 用的 stub 服务(`-Mode dsh` 像 DSH;`-Mode other` 是无关网页程序),会写 `.testdata` 归属标记 |
| `scripts/test-slow-server.ps1` | 场景 E 用:先绑端口回 401,`-DelaySeconds` 秒后才打印带 token 的 URL(复现真实启动顺序) |
| `assets/icon.ico` | 程序图标 |
| `README.md` / `README.en.md` | 面向用户的功能说明(中/英):行为表、配置文件、Win10/11 兼容性、故障排查、测试开关 |

`src/main.rs` 主流程:`SingleInstance`(命名互斥量) → 载入 `dsh-launcher.conf`(exe 同目录,缺失则按
模板生成;命令/参数/首选端口/复用标记 `ui_marker`/URL 标记/超时都在此) → 轮转过大日志、清理已无主的
`profiles\run-*`(按其中的 `launcher.pid` 判断归属) → HTTP 探测首选端口:返回 200 **且正文含
`ui_marker`**(默认 `DeepSeek Harness`)才复用;需令牌、非 HTTP、或不像 DSH 都改用 `--port 0` 另起自己的
实例 → 按"完整命令 → 命令 + 空闲端口 → 仅命令"依次尝试(`cmd.exe` + `raw_arg` 原样传命令行,避免
Rust 把引号转义成 `\"`;失败立即换下一档) → `wait_ready`(解析 dsh 打印的带 token URL;**端口先通
不是失败**,裸地址只在真能 200/303 时才被接受,否则一直等到超时)→ 用独立临时 profile 按 `window_mode`
开 Chrome(默认 `normal`:普通窗口,有地址栏;`app`:`--app=<url>` 应用窗口,无地址栏)→ 等窗口关闭 →
只结束自己启动的服务(先 `try_wait` 确认它还活着再 `taskkill /T`,另有 Job Object `KILL_ON_JOB_CLOSE`
兜底)→ 删临时 profile → 退出。

运行期数据:`%LOCALAPPDATA%\dsh-launcher\` —— `launcher.log` 是本程序日志,`server.log` 是 dsh
子进程的 stdout+stderr 合并。

## 常用命令

```powershell
cargo build --release            # 产出 target\release\dsh-launcher.exe(需 rc.exe 才有图标)
cargo build --release --offline  # 强制离线
cargo check
```

发布后**手动**把 `target\release\dsh-launcher.exe` 复制成仓库根目录的 `dsh-launcher.exe`
(那份是给双击用的便捷副本,已被 `.gitignore` 忽略,不会自动同步,容易忘记)。

**回归测试**:无单元测试(`tests/` 不存在,`src/main.rs` 内也没有 `#[test]`),端到端回归脚本是
`scripts/regression.ps1`(A/B/C/D/E 五套、32 项判据,约 4 分钟):

```powershell
cargo build --release
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/regression.ps1
```

它设置 `DSH_SKIP_SINGLE_INSTANCE=1`,所以**本机正在使用的启动器实例不必关掉**;它只碰自己的
`.testdata\` 数据目录与首选端口(默认 3099)。

## 项目约定

- **坚持零依赖**。新增 Win32 能力时手写 `extern "system"` 声明 + `#[repr(C)]` 布局,不引入 crate。
- **绝不出现控制台窗口**:所有子进程必须带 `CREATE_NO_WINDOW`;诊断信息写日志文件,不要指望 stdout。
- **面向用户的报错用中文 `MessageBoxW`**,并遵守 `DSH_NO_UI=1` 时静默(自动化回归依赖它)。
- **日志是追加写的**:任何"读取子进程输出"的逻辑都必须先记录文件偏移(见 `start_server` 返回的
  `log_from`),只解析本次运行新增的字节,否则会读到上一次运行的旧数据。
- **`dsh web` 的鉴权不可绕过、也不可推测**:launch token 是 `randomBytes(32)`,只存在那个 node
  进程内存里,不落盘、无相关环境变量、无 `--no-auth` 参数;带 token 的 URL **只会打印在它的
  stdout**。启动器必须从 `server.log` 里解析这行 URL 再交给浏览器,裸地址一律 401。
- **端口必须显式传给 dsh**:启动器探测的端口和浏览器要访问的端口,必须是 dsh 真正绑定的那个。
  因此 `npx … web` 命令行里始终带 `--port <端口>`,不要依赖 dsh 自己的默认值(历史上 `DSH_PORT`
  只改了探测、没传给 dsh,是个失效开关)。
- **只复用"确实是 DSH"的服务,绝不杀别人的服务**:`dsh web` 的访问令牌只在那一个进程的内存里,别人启的
  实例我们无法驱动(裸地址必 401)。因此:首选端口上的服务须对裸地址返回 200 **且正文含 `ui_marker`**
  → 直接复用,关窗时不结束它;若返回 401、非 HTTP、或正文不像 DSH(别的网页程序)→ 改用 `--port 0`
  另起自己的实例,只结束自己启动的那个。
- **"猜"不如"读",且所有 DSH 假设都要可配置**:端口与 URL 一律取自 dsh 自己的输出(尤其那行带 token
  的 URL),不要把端口/URL/参数硬编码进逻辑;命令、附加参数、首选端口、URL 标记、超时都要能从
  `dsh-launcher.conf` 改。这样 DSH 更新时改配置即可,无需重编译——这是本项目的核心设计目标。
- **端口先通 ≠ 启动失败**:dsh 会先 `listen` 再在 loader settle 之后打印带 token 的 URL,中间那段
  "端口可连 + 裸地址 401" 是正常中间态(本机实测 spawn→ready 约 10 秒)。`wait_ready` 里的 5 秒宽限
  只能用来"接受真的可用的裸地址",非 200/303 一律继续等到超时,否则会误杀刚起的服务。回归场景 E
  专门盯这条。
- **配置文件模板只写 ASCII**:用户会用记事本编辑 `dsh-launcher.conf`,若模板含中文注释而用户另存为
  ANSI,就得靠 `String::from_utf8_lossy` + 去 BOM 兜底(已实现)。新增模板文字请保持 ASCII。
- **地址栏的"安装"图标无法在普通窗口里去掉了**:`dsh-web-frontend/dist/manifest.webmanifest` 让页面成为
  "可安装的 Web 应用",Chrome 便在**普通窗口的地址栏**加"安装"入口。已证伪的尝试:内容设置
  `default_content_setting_values.web_app_installation = 2`(Chrome 保留了该设置,图标依旧)、
  `--disable-features=WebAppInstallation`(逐行像素与基线完全一致)、`WebAppInstallationPromo`、
  `DesktopPWAInstallPromotionML`、`PwaInstall` 及其组合(均无变化)。因此 `window_mode` 是取舍:
  **默认 `normal`**(有地址栏,也有该图标),`app`(`--app=<url>`:无地址栏/标签栏 → 图标无从出现)。
  用户明确要地址栏,**不要再把默认改回 app**;也**不要**去改 DSH 的前端文件。
  教训:凡"预置偏好 / 隐藏 UI"类改动,必须用**用户可见的结果**验证,不能只看键是否被保留。
- 注释与标识符用英文,面向用户的文案用中文。
- 保持 `src/main.rs` 现有写法:单文件、`fn` 按主流程顺序排布、少抽象。

## 相关文档

- `HANDOFF.md`:当前任务、测试命令、状态、TODO、最新 commit —— 接手/新会话先读它。
- `README.md`:用户视角的功能与排障(含 `DSH_*` 测试开关表)。
