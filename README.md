# DSH One-Click Launcher(DeepSeek Harness 一键启动器)

[![Release](https://img.shields.io/github/v/release/zhubingzuo/dsh-oneclick-launcher)](https://github.com/zhubingzuo/dsh-oneclick-launcher/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**简体中文** | [English](README.en.md)

一个 **Windows 双击即用**的启动器:静默启动 DeepSeek Harness 网页版,打开一个
**全新的干净 Chrome 窗口**进入 DSH 页面,全程**不弹出任何黑色命令提示符窗口**;
关闭该 Chrome 窗口后自动停止后台服务并退出,不留残留进程。

> 它是 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)(`@deepseek-ai/dsh`)
> 网页版的非官方启动器,让"打开 DSH 页面"变成双击一下的事。
>
> 设计目标是**跟随 DSH 版本演进**:地址从 dsh 自己的输出里读(不猜端口、不硬编码 URL),
> 启动方式写在可编辑的配置文件里——常规的 DSH 更新**不需要重新编译**本启动器。

## 下载 / 成品位置

从 [最新 Release](https://github.com/zhubingzuo/dsh-oneclick-launcher/releases/latest) 直接下载
`dsh-launcher.exe`,或自行构建(见 [从源码构建](#从源码构建零第三方依赖可离线)):

```
target\release\dsh-launcher.exe
```

把它复制到任意位置(桌面、U 盘等)都可以双击运行。首次运行会在 exe 同目录生成
`dsh-launcher.conf`(见下文)。

## 功能特性

- **双击即用、无控制台**:自动启动 DSH 网页服务;exe 为 GUI 子系统,所有子进程用
  `CREATE_NO_WINDOW` 创建,全程没有任何黑框
- **只开一个干净窗口**:dsh web 自身默认还会再打开一次默认浏览器,启动器用官方
  `--no-open` 参数关掉它,浏览器只由启动器打开**一个**窗口
- **全新独立 Chrome 窗口**:使用**独立的临时配置目录**(`--user-data-dir`)启动,
  配合 `--new-window`、`--no-first-run`、`--disable-session-crashed-bubble` 等参数——
  窗口带地址栏、里面**只有 DSH 一个标签页**,不混入你日常 Chrome 的任何网站/书签/扩展/登录
- **关窗即停、无残留**:关闭启动器打开的 Chrome 窗口 → 自动结束自己启动的 dsh web 进程树
  (Windows Job Object `KILL_ON_JOB_CLOSE` + `taskkill` 进程树双重保障)
- **不干扰已有服务**:首选端口上已有服务时——若它**不需要令牌**(老版本 DSH)就直接复用;
  若它**需要令牌**(新版 DSH 的页面令牌只存在于那个进程的内存里,外部无法驱动)则改用
  系统分配的空闲端口启动自己的实例。**任何情况下都不会结束别人的服务**
- **抗版本变化**:网页地址取自 dsh 打印的那行带令牌 URL;命令、参数、端口、输出标记与超时
  全部可在配置文件中修改,DSH 改动启动方式时**改文本即可,无需重新编译**
- **单实例**:重复双击不会重复启动
- **出错有提示**:任何失败弹中文错误框,并给出日志位置与可修改的配置项

## 为什么 DSH 更新后通常不用重新编译

| 可能的版本变化 | 启动器的应对 |
| --- | --- |
| 页面端口变了 | 端口不写死:始终使用 dsh 打印的 URL(必要时代 `--port 0` 让系统分配) |
| 页面加了令牌/鉴权 | 令牌 URL 从 dsh 的 stdout 读取(裸地址会 401,启动器不使用裸地址) |
| 输出格式/提示文字变了 | 先按配置的 `url_marker` 解析;失败再匹配"带 token 的本地地址"两种形态 |
| 参数改名/移除(如 `--no-open`) | 依次尝试"完整命令 → 去掉 `--port` → 仅命令",前一个失败立即换下一个 |
| 启动子命令或包名变了 | 改 `dsh-launcher.conf` 里的 `command` / `extra_args`,不用重编译 |

## 配置文件 `dsh-launcher.conf`

首次运行时自动生成在 exe 同目录(该目录不可写时改放 `%LOCALAPPDATA%\dsh-launcher\`)。
纯文本、带注释,改完立即生效:

```ini
command = npx --yes @deepseek-ai/dsh web    # 启动命令
extra_args = --no-open                      # 首次尝试附加的参数
port = 3080                                 # 首选端口;0 = 总让 dsh 自己挑
ui_marker = DeepSeek Harness                # 复用已有页面时必须包含的文字
url_marker = dsh web:                       # 输出中标记网页地址的文字
timeout_secs = 300                          # 就绪等待上限
window_mode = normal                        # normal = 带地址栏的普通窗口(默认);app = 无地址栏的应用窗口
```

也可以用环境变量 `DSH_LAUNCHER_CONFIG` 指定配置文件路径。

> `ui_marker` 是防误用的保险:首选端口上若有**别的**本地网页程序(而不是 DSH),它即使返回 200
> 也不会被采用,启动器会另起自己的实例。把它设为空值则恢复"任何 200 都复用"的宽松行为。
>
> `window_mode` 决定窗口形态,也就是"要不要地址栏"与"要不要那个'安装'图标"之间的取舍:
> - **`normal`(默认)**:普通 Chrome 窗口,有地址栏和标签栏。DSH 网页自带
>   `manifest.webmanifest`(一份"可安装的 Web 应用"声明),所以 Chrome 会在地址栏里显示蓝色的
>   "安装"入口。**这个图标在普通窗口里去不掉**:实测把配置档的"Web 应用安装"内容设置设为"阻止",
>   以及 `--disable-features=WebAppInstallation`、`WebAppInstallationPromo`、
>   `DesktopPWAInstallPromotionML`、`PwaInstall` 等组合,图标都依旧(Chrome 没提供这样的开关)。
> - **`app`**:Chrome 应用窗口,**没有地址栏、没有标签栏**,因此那个图标无从出现;代价是也没有地址栏。

## 行为细节

| 场景 | 程序行为 |
| --- | --- |
| 首选端口空闲 | 启动 `npx --yes @deepseek-ai/dsh web --no-open --port 3080` → 读它打印的带令牌 URL → 打开全新 Chrome 窗口 |
| 首选端口上是**老版本**(无需令牌)的 DSH | **直接复用**该服务,只开浏览器;关窗后程序退出,**不结束**那个服务 |
| 首选端口上是**别的网页程序**(非 DSH) | 识别出它不像 DSH → 改用 `--port 0` 另起自己的实例,**不采用也不结束**那个程序 |
| 首选端口上是**新版本**(需令牌)的 DSH | 改用 `--port 0` 让系统给一个空闲端口,启动**自己的** dsh web;关窗后只结束自己启动的服务 |
| 双击时已有实例在运行 | 单实例锁生效,第二个实例直接退出 |
| 用户关闭打开的 Chrome 窗口 | 结束自己启动的 dsh web 进程树 → 清理临时配置 → 程序退出 |
| Node.js 缺失 / Chrome 缺失 / 启动失败 / 超时 | 中文错误框 + 详细日志指引 + 提示可修改的配置项 |

## Windows 10 / 11 兼容性

- **支持范围**:Windows 10(x64)与 Windows 11(x64)。程序为 **64 位**构建,32 位 Windows
  不支持;Windows 11 on ARM 可通过系统自带的 x64 模拟运行。
- **只用系统自带 API**:命名互斥量、Job Object(作业对象)、`MessageBoxW` 都是 Windows 7/8
  起就存在的接口,不依赖任何 Windows 11 专属特性。
- **单文件、零运行时依赖**:不需要安装 .NET、VC++ 运行库或任何 DLL(Rust MSVC 静态链接)。
- **首次运行会有 SmartScreen 提示**:从浏览器下载的未签名 exe 会显示"Windows 已保护你的电脑",
  点"更多信息"→"仍要运行"即可。若被安全软件拦截,需要自行放行——程序只做三件事:执行
  `npx … dsh web`、启动 Chrome、按 pid 结束**自己启动的**进程树。
- **前置条件**:Node.js(含 `npx`)与 Google Chrome;两者在 Windows 10/11 上均正常支持。
- **中文/非 ASCII 路径没问题**:exe 路径、`%LOCALAPPDATA%`(如 `C:\Users\中文名\…`)都以
  UTF-16 传递给系统调用;配置文件是 UTF-8,万一被另存成 ANSI 也只是回退默认值,不会崩。
- **多用户/多会话**:单实例锁按登录会话隔离,不同 Windows 用户各自双击互不干扰。
- **实测情况**:在 Windows 11(build 26200,x64)上通过 26 项端到端回归;Windows 10 未能在
  本机实测,但所用 API 与依赖均为 Windows 10 起可用,无版本专属调用。

## 从源码构建(零第三方依赖,可离线)

需要:Windows + Rust(msvc 工具链)。图标嵌入使用 Windows SDK 自带的 `rc.exe`;
找不到 rc.exe 时仍能构建,只是 exe 没有图标。

```powershell
cargo build --release             # 需要 Windows SDK 的 rc.exe 以嵌入图标
cargo build --release --offline   # 或强制离线
```

## 回归测试

无单元测试;用 `scripts/regression.ps1` 跑五套端到端场景、共 32 项判据(先 `cargo build --release`):

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/regression.ps1
```

- **A** 首选端口空闲 → 启动器应自己起服务、读到带令牌 URL、关窗后停服务并释放端口
- **B** 首选端口已被**带令牌**的 dsh 占用 → 应改用 `--port 0` 另起实例,且原服务毫发无损
- **C** 首选端口已被**无需令牌、且看起来就是 DSH** 的页面占用 → 应直接复用,不另起、不结束它
- **D** 首选端口已被**无关的网页程序**占用 → 应拒绝采用它(即使返回 200),改用 `--port 0` 另起实例
- **E** 服务先绑端口、先回 401、**几秒后才**打印带令牌 URL(真实 dsh 的启动顺序)→ 启动器应继续等待而不是误判失败并杀掉它

脚本借助 `DSH_*` 测试开关运行,因此可以与本机正在使用的启动器实例并存。

## 程序图标

`assets/icon.ico` 基于 DSH 官方鲸鱼 favicon 生成:
深蓝圆角底 + 白色鲸鱼,包含 16–256 共 9 个尺寸。重新生成:

```powershell
# 指定 DSH 的 favicon.svg 路径(示例;可从 @deepseek-ai/dsh 包内取得)
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/gen_icon.ps1 -SvgPath "C:\...\favicon.svg"
```

不传 `-SvgPath` 时,脚本会尝试在 npm 全局安装目录
(`%APPDATA%\npm\node_modules\@deepseek-ai\dsh\...`) 下自动寻找 favicon.svg。

## 故障排查与日志

任何失败都会在错误框中给出日志位置:

- 运行日志:`%LOCALAPPDATA%\dsh-launcher\launcher.log`
- 服务输出:`%LOCALAPPDATA%\dsh-launcher\server.log`(dsh 的 stdout+stderr)

| 现象 | 建议 |
| --- | --- |
| 弹窗"无法启动 DSH 服务" | 确认已安装 Node.js 并加入 PATH;首次运行可能需要联网下载 dsh 包 |
| 弹窗"无法打开 Chrome" | 确认已安装 Google Chrome;也可手动打开日志中的 URL |
| 弹窗提示"无法取得访问令牌" | DSH 更新后改了输出格式:检查配置文件里的 `url_marker` |
| 启动超时 | 查看 `server.log`,或手动在终端运行配置里的 `command` 看详细错误;必要时调大 `timeout_secs` |
| 看到多个浏览器窗口 | 确认运行的是最新版(旧版未加 `--no-open` 会让 dsh web 自己再开一个) |
| 地址栏出现蓝色"**安装**"图标 | 那是 Chrome 认为 DSH 页面可安装(网页自带 Web App Manifest)。**普通窗口里去不掉**——已实测:内容设置"Web 应用安装 → 阻止"无效,`--disable-features=WebAppInstallation` / `WebAppInstallationPromo` / `DesktopPWAInstallPromotionML` / `PwaInstall` 等组合也无效。想彻底不看到它,就把配置改成 `window_mode = app`(应用窗口,**代价是没有地址栏**);否则忽略它即可,它只是个安装入口,不影响使用 |

## 目录结构

```
dsh-launcher/
├─ assets/icon.ico              # 程序图标(多尺寸)
├─ scripts/gen_icon.ps1         # 图标生成脚本
├─ scripts/regression.ps1       # A/B/C/D/E 五套端到端回归测试(32 项判据)
├─ scripts/test-stub-server.ps1 # 回归测试用的 stub 服务(C/D 场景)
├─ scripts/test-slow-server.ps1 # 回归测试用的慢启动服务(E 场景)
├─ build.rs                     # 零依赖嵌入 .ico(调用 rc.exe)
├─ src/main.rs                  # 主程序(纯 std + 少量手写 Win32 FFI)
├─ AGENTS.md / HANDOFF.md       # 项目约定 / 交接说明
├─ Cargo.toml / Cargo.lock
├─ LICENSE
├─ README.md                    # 简体中文(本文件)
└─ README.en.md                 # English
```

## 内部测试开关(普通使用无需关心)

| 环境变量 | 作用 |
| --- | --- |
| `DSH_LAUNCHER_CONFIG` | 指定配置文件路径(默认 exe 同目录的 `dsh-launcher.conf`) |
| `DSH_DATA_DIR` | 日志/临时配置目录(默认 `%LOCALAPPDATA%\dsh-launcher`) |
| `DSH_PORT` | 覆盖首选端口(默认取自配置文件,3080) |
| `DSH_SERVER_EXTRA` | 用自定义命令完全代替配置里的 `command`(回归测试用) |
| `DSH_FAKE_CHROME` | 用 ping 代替真实 Chrome,模拟"浏览器打开约 N 秒后关闭" |
| `DSH_NO_UI` | `1` 时禁止弹窗,只写日志 |
| `DSH_READY_TIMEOUT_SECS` | 覆盖就绪等待秒数 |
| `DSH_CHROME` | 指定 chrome.exe 路径 |
| `DSH_SKIP_SINGLE_INSTANCE` | `1` 时跳过单实例检查(便于与正在运行的实例并存跑回归) |

## 声明

本项目是 DSH 的**非官方**启动器,与 DeepSeek 官方无隶属关系;
DeepSeek Harness 及其图标等权利归其原始权利人所有。

## 许可证

[MIT](LICENSE) © 2026 zhubingzuo
