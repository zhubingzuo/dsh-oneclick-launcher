# DSH One-Click Launcher(DeepSeek Harness 一键启动器)

一个 **Windows 双击即用**的启动器:静默启动 DeepSeek Harness 网页版,打开一个
**全新的干净 Chrome 窗口**进入 DSH 页面,全程**不弹出任何黑色命令提示符窗口**;
关闭该 Chrome 窗口后自动停止后台服务并退出,不留残留进程。

> 它是 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)(`@deepseek-ai/dsh`)
> 网页版的非官方启动器,让"打开 DSH 页面"变成双击一下的事。
>
> 设计目标是**跟随 DSH 版本演进**:地址从 dsh 自己的输出里读(不猜端口、不硬编码 URL),
> 启动方式写在可编辑的配置文件里——常规的 DSH 更新**不需要重新编译**本启动器。

## 成品位置

双击运行(Release 版,已内嵌程序图标,无控制台窗口):

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
url_marker = dsh web:                       # 输出中标记网页地址的文字
timeout_secs = 300                          # 就绪等待上限
```

也可以用环境变量 `DSH_LAUNCHER_CONFIG` 指定配置文件路径。

## 行为细节

| 场景 | 程序行为 |
| --- | --- |
| 首选端口空闲 | 启动 `npx --yes @deepseek-ai/dsh web --no-open --port 3080` → 读它打印的带令牌 URL → 打开全新 Chrome 窗口 |
| 首选端口上是**老版本**(无需令牌)的服务 | **直接复用**该服务,只开浏览器;关窗后程序退出,**不结束**那个服务 |
| 首选端口上是**新版本**(需令牌)或其它程序 | 改用 `--port 0` 让系统给一个空闲端口,启动**自己的** dsh web;关窗后只结束自己启动的服务 |
| 双击时已有实例在运行 | 单实例锁生效,第二个实例直接退出 |
| 用户关闭打开的 Chrome 窗口 | 结束自己启动的 dsh web 进程树 → 清理临时配置 → 程序退出 |
| Node.js 缺失 / Chrome 缺失 / 启动失败 / 超时 | 中文错误框 + 详细日志指引 + 提示可修改的配置项 |

## 从源码构建(零第三方依赖,可离线)

需要:Windows + Rust(msvc 工具链)。图标嵌入使用 Windows SDK 自带的 `rc.exe`;
找不到 rc.exe 时仍能构建,只是 exe 没有图标。

```powershell
cargo build --release             # 需要 Windows SDK 的 rc.exe 以嵌入图标
cargo build --release --offline   # 或强制离线
```

## 回归测试

无单元测试;用 `scripts/regression.ps1` 跑三套端到端场景(先 `cargo build --release`):

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/regression.ps1
```

- **A** 首选端口空闲 → 启动器应自己起服务、读到带令牌 URL、关窗后停服务并释放端口
- **B** 首选端口已被**带令牌**的 dsh 占用 → 应改用 `--port 0` 另起实例,且原服务毫发无损
- **C** 首选端口已被**无需令牌**的服务占用 → 应直接复用,不另起、不结束它

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

## 目录结构

```
dsh-launcher/
├─ assets/icon.ico              # 程序图标(多尺寸)
├─ scripts/gen_icon.ps1         # 图标生成脚本
├─ scripts/regression.ps1       # A/B/C 三套端到端回归测试
├─ scripts/test-stub-server.ps1 # 回归测试用的无令牌 stub 服务
├─ build.rs                     # 零依赖嵌入 .ico(调用 rc.exe)
├─ src/main.rs                  # 主程序(纯 std + 少量手写 Win32 FFI)
├─ AGENTS.md / HANDOFF.md       # 项目约定 / 交接说明
├─ Cargo.toml / Cargo.lock
└─ README.md
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

## 许可证

[MIT](LICENSE) © 2026 zhubingzuo
