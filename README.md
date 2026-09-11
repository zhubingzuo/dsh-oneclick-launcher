# DSH One-Click Launcher(DeepSeek Harness 一键启动器)

一个 **Windows 双击即用**的启动器:静默执行 `npx @deepseek-ai/dsh web`,打开一个
**全新的干净 Chrome 窗口**进入 `http://127.0.0.1:3080/`,全程**不弹出任何黑色命令提示符窗口**;
关闭该 Chrome 窗口后自动停止后台服务并退出,不留残留进程。

> 它是 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)(`@deepseek-ai/dsh`)
> 网页版的非官方启动器,让"打开 DSH 页面"变成双击一下的事。

## 成品位置

双击运行(Release 版,已内嵌程序图标,无控制台窗口):

```
target\release\dsh-launcher.exe
```

把它复制到任意位置(桌面、U 盘等)都可以双击运行。

## 功能特性

- **双击即用**:自动执行 `npx @deepseek-ai/dsh web`,无需手动打开终端
- **绝无黑色控制台窗口**:exe 为 GUI 子系统,所有子进程用 `CREATE_NO_WINDOW` 创建
- **只开一个干净窗口**:dsh web 自身默认还会再打开一次默认浏览器,启动器用官方
  `--no-open` 参数关掉它,浏览器只由启动器打开**一个**窗口
- **全新独立 Chrome 窗口**:使用**独立的临时配置目录**(`--user-data-dir`)启动,
  配合 `--new-window`、`--no-first-run`、`--disable-session-crashed-bubble` 等参数——
  窗口带地址栏、里面**只有 DSH 一个标签页**,不混入你日常 Chrome 的任何网站/书签/扩展/登录
- **关窗即停、无残留**:关闭启动器打开的 Chrome 窗口 → 自动结束 dsh web 进程树并退出
  (Windows Job Object `KILL_ON_JOB_CLOSE` + `taskkill` 进程树双重保障)
- **不抢端口、也不复用别人的服务**:3080 被占用(无论是别的 dsh web 还是其它程序)时,**自动改用系统分配的空闲端口**启动自己的 dsh web;绝不会结束或干扰原来那个服务
- **单实例**:重复双击不会重复启动
- **出错有提示**:任何失败弹中文错误框,并给出日志位置

## 行为细节

| 场景 | 程序行为 |
|---|---|
| 3080 端口空闲 | 静默启动 `npx --yes @deepseek-ai/dsh web --no-open --port 3080` → 等就绪 → 打开全新 Chrome 窗口 |
| 3080 已被占用(另一 dsh web 或其它程序) | 自动改用 `--port 0` 让系统给一个空闲端口启动**自己的** dsh web,并打开对应地址;关窗后只结束自己启动的服务,**不影响**原有服务 |
| 双击时已有实例在运行 | 单实例锁生效,第二个实例直接退出 |
| 用户关闭打开的 Chrome 窗口 | 结束自己启动的 dsh web 进程树 → 清理临时配置 → 程序退出 |
| Node.js 缺失 / Chrome 缺失 / 启动失败 / 超时 | 中文错误框 + 详细日志指引 |

## 从源码构建(零第三方依赖,可离线)

需要:Windows + Rust(msvc 工具链)。图标嵌入使用 Windows SDK 自带的 `rc.exe`;
找不到 rc.exe 时仍能构建,只是 exe 没有图标。

```powershell
cargo build --release          # 需要 Windows SDK 的 rc.exe 以嵌入图标
cargo build --release --offline   # 或强制离线
```

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
- 服务输出:`%LOCALAPPDATA%\dsh-launcher\server.log`

| 现象 | 建议 |
|---|---|
| 弹窗"无法启动 npx …" | 确认已安装 Node.js 并加入 PATH;首次运行可能需要联网下载 dsh 包 |
| 弹窗"无法打开 Chrome" | 确认已安装 Google Chrome;也可手动打开日志中的 URL |
| 启动超时 | 查看 `server.log`,或手动在终端运行 `npx @deepseek-ai/dsh web` 看详细错误 |
| 看到多个浏览器窗口 | 确认运行的是最新版(旧版未加 `--no-open` 会让 dsh web 自己再开一个) |

## 目录结构

```
dsh-launcher/
├─ assets/icon.ico         # 程序图标(多尺寸)
├─ scripts/gen_icon.ps1    # 图标生成脚本
├─ build.rs                # 零依赖嵌入 .ico(调用 rc.exe)
├─ src/main.rs             # 主程序(纯 std + 少量手写 Win32 FFI)
├─ Cargo.toml / Cargo.lock
└─ README.md
```

## 内部测试开关(普通使用无需关心)

| 环境变量 | 作用 |
| --- | --- |
| `DSH_DATA_DIR` | 日志/临时配置目录(默认 `%LOCALAPPDATA%\dsh-launcher`) |
| `DSH_PORT` | 首选端口(默认 `3080`)。会作为 `--port` 传给 dsh;该端口被占用时自动改用空闲端口 |
| `DSH_SERVER_EXTRA` | 用自定义命令代替 `npx ... web`(自动化测试用) |
| `DSH_FAKE_CHROME` | 用 ping 代替真实 Chrome 模拟"浏览器打开 N 秒后关闭" |
| `DSH_NO_UI` | `1` 时禁止弹窗,只写日志 |
| `DSH_READY_TIMEOUT_SECS` | 就绪等待秒数(默认 300) |
| `DSH_CHROME` | 指定 chrome.exe 路径 |

## 许可证

[MIT](LICENSE) © 2026 zhubingzuo
