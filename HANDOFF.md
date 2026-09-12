# HANDOFF

> 接手前先读本文件。改动代码后请更新它;更早的进展见 `docs/LOG.md`。
## 任务目标

让 Windows(10/11 x64)上双击 `dsh-launcher.exe` 一键打开 DSH(DeepSeek Harness)网页界面:
静默启动 dsh web、开干净独立 Chrome 窗口、关窗即停、无残留、不干扰别人的服务;并且 **DSH 升级后
通常不需要重新编译本启动器**(地址取自 dsh 自身输出,启动方式写在外部配置文件里)。
功能已完成、经独立代码评审并修复其发现,处于维护状态。
## 测试命令

无单元测试,回归 = `scripts/regression.ps1`(A/B/C/D/E 五套、32 项判据,约 4 分钟):

```powershell
cargo build --release
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/regression.ps1
```

- **A** 首选端口(3099)空闲 → 自己起服务(`--port 3099`)、读到带 token 的 URL、关窗后
  `stopping server tree` 且端口释放。
- **B** 3099 被**带 token**的 dsh 占用 → 识别 `HTTP 401`、改用 `--port 0`,原服务全程与退出后都在。
- **C** 3099 被**无 token 且像 DSH** 的页面占用 → 直接复用(不出现 `server command:` /
  `stopping server tree`),stub 仍存活。
- **D** 3099 被**无关网页程序**占用 → 拒绝采用(日志 `does not contain "DeepSeek Harness"`),
  改用 `--port 0`,那个程序仍存活。
- **E** 服务**先绑端口回 401、8 秒后才打印带 token 的 URL**(真实 dsh 顺序)→ 必须继续等待
  (`answered 401; waiting for the tokenized launch URL`),不得误判失败,最终用**迟到的 URL** 打开。

脚本设置 `DSH_SKIP_SINGLE_INSTANCE=1`,**本机正在使用的启动器实例不必关掉**;它只用 `.testdata\`
数据目录与 3099/3098 端口,且每个 stub 都会写归属标记文件,避免"别人的服务恰好占着端口"造成假通过。
## 本次完成及验证结果

- 独立子代理代码评审(只读)发现 **1 Critical + 5 Important + 若干 Minor**,已逐条处理,见下节。
- 验证(2026-09-12):`cargo build --release --offline` 通过、`cargo clippy --release --all-targets`
  **零警告**;**回归 32/32 PASS**(含新增场景 E);另单独验证 `raw_arg` 修复(带引号命令的
  `server.log` 出现预期输出)。
- 期间未结束任何用户正在使用的进程:承载本会话的 launcher(pid 28400)与 3080 服务全程运行。
## 本次修复清单(评审发现 → 处理)

1. **Critical**:`wait_ready` 的 5 秒宽限原会直接判 `NoLaunchUrl` 并 `kill_tree` 刚起的服务;
   dsh 实测 spawn→ready 约 10 秒,Win10 老机器更慢。现改为:宽限只用于"接受真能 200/303 的裸地址",
   其它状态一律继续等到 `deadline`;`NoLaunchUrl` 仅在超时且端口确有服务时才报。回归场景 E 覆盖。
2. **Important**:`find_launch_url` 的 marker 分支原会把**不带 token 的 URL** 交给浏览器。
   现要求"带 `token=` **或**裸地址实测 200/303"才接受,否则继续等;同时只接受
   `127.0.0.1/localhost`,不会把 `(LAN: …)` 里的地址交给浏览器。
3. **Important**:`cmd /C` 原用 `args(["/C", cmdline])`,Rust 会把引号转义成 `\"` 而 cmd 不认,
   于是配置里带引号的 `command`(如 `"C:\Program Files\nodejs\npx.cmd" web`)必然启动失败。
   改用 `CommandExt::raw_arg` 原样传命令行,并已实测。
4. **Important**:profile 目录泄漏(评审实测 13 个 / 653 MB;`ServerDied` 分支漏清理、重试仅 2 秒)。
   现在退出重试 15×500 ms,`ServerDied` 分支也清理;`profiles\run-*` 里写入 `launcher.pid`,
   清理时用 `OpenProcess` 判断"属主仍活着"就跳过,彻底避免删到别的会话正在用的 profile。
5. **Important**:配置非 UTF-8(记事本 ANSI)时静默失效。现改为按字节读 + 去 UTF-8 BOM +
   `from_utf8_lossy`,ASCII 键值仍可用。
6. **Important**:仓库根目录的便捷 exe 是 1.0 旧版(308,224 字节,且被运行中实例锁定无法覆盖)。
   新版为 `target\release\dsh-launcher.exe`(1.3.0);已额外复制一份到**仓库外**
   `K:\BaiduSyncdisk\Rust\dsh\dsh-launcher-1.3.0.exe` 供直接双击,根目录旧副本待实例关闭后覆盖。
7. **Minor**:回退链第 2 档原会丢掉 `--port`(dsh 默认 3080,正是用户最可能占用的端口)。现改为
   "完整命令 → 命令 + `--port 0` → 仅命令";日志读改 seek(不再整文件读);启动失败弹窗补充
   "日志目录不可写"这一可能;`build.rs` 的 rc.exe 探测重写(SDK 目录/PATH)、加 `/c 65001`
   (项目路径含中文也能嵌图标)、修正文案笔误。
8. **用户反馈:地址栏出现蓝色"安装"图标**。根因是 DSH 前端自带
   `dsh-web-frontend/dist/manifest.webmanifest`(`display: fullscreen`),Chrome 判定页面"可安装"。
   启动器打开的 profile 是临时的、关窗即删,安装没有意义,因此新增 `seed_chrome_prefs`:在 Chrome
   启动前写入该 profile 的 `Default\Preferences`
   (`profile.default_content_setting_values.web_app_installation = 2`,即"Web 应用安装 = 阻止")。
   已用 headless Chrome 实测该键被 Chrome 保留(156 → 9005 字节,与自身默认合并),说明它被认可;
   配置项 `block_web_app_install = false` 可恢复图标。**没有**去改 DSH 的前端文件。
## 下一步 TODO

1. 关掉正在运行的实例后,**手动**把 `target\release\dsh-launcher.exe` 覆盖到仓库根目录的便捷副本
   `dsh-launcher.exe`(它当前被运行中的实例锁定,仍是 1.0 时代的旧版)。仓库/Release 里已是新版。
2. 已推送 `origin/main`;发版流程:改 `Cargo.toml` 版本 → 构建 → 跑回归 → 提交推送 → 建 tag/Release
   并附上 `target\release\dsh-launcher.exe`。
3. 若某版 dsh 既不打印带 token 的 URL、裸地址也不可用,现在会在**超时后**明确报错并指向 `url_marker`
   (不再有"5 秒即失败"的误判),这是有意行为,不要改回提前失败。
4. 可选演进:复用判定从"正文含 `ui_marker`"升级为更硬的信号(DSH 专有响应头/资源路径);
   给 `.conf` 增加"额外尝试命令列表";把 `server.log` 改成每运行一个文件(避免多会话互读 token)。
## 当前的坑

- **单实例锁会静默退出**:有实例在跑时新实例只写一行日志就退出;跑回归务必带
  `DSH_SKIP_SINGLE_INSTANCE=1`。
- **裸地址必失败**:`dsh web` 的 UI 要进程级 launch token(只在其内存、不落盘、无开关可关),
  裸地址恒 `401`。启动器只把 dsh 打印的带 token URL 交给浏览器。
- **端口先通不是失败**:见上节第 1 条与 AGENTS.md 对应约定。
- **日志追加写**:解析 dsh 输出必须只用 `log_from` 之后的新增字节,否则会复用上一次的旧 token。
- **端口探测超时仅 400ms**;`netstat` 里的 `TIME_WAIT` 不是 LISTENING,不参与判定。
- **仓库根目录的便捷 exe 会被运行中的实例锁定**,无法覆盖;必须先关窗口。
- `DSH_FAKE_CHROME=N` 实际是 `ping -n 2N`(约 2N 秒)。README 已按"约 N 秒"描述。
- 活的 launch token 会写进 `launcher.log` / `server.log`(同用户可读);这是有意保留的诊断信息。
## 下次恢复需打开的关键文件

`src/main.rs`(全部逻辑:`LaunchConfig::load`/`find_launch_url`/`http_get`/`wait_ready`/
`build_attempts`/`start_server`/`prune_profiles`/`run`)、`README.md`(中)+`README.en.md`(英)、
`AGENTS.md`(架构与约定)、`scripts/regression.ps1` + 两个 stub 脚本、`docs/LOG.md`(会话存档)。
## 最新 commit

本次修复提交见 `git log`(父提交为 `b174af0` 中英双语文档)。更早:

```
b174af0  docs: 中英双语介绍(README.en.md + 双向语言链接 + 徽章)
0686272  docs: 交接状态更新(已推送、已发布 v1.1.0)
643154f  docs: 会话存档(抗版本变化改造与 A/B/C 回归结果)
f03321e  feat: 抗 DSH 版本变化的启动方式(外部配置 + 复用已有服务 + 参数回退)
c4601f4  修复:用 dsh 打印的 token URL 打开浏览器,并修正 DSH_PORT 与端口占用处理
316498c  Initial release: DSH one-click launcher for Windows
```
