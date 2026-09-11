# HANDOFF

> 接手前先读本文件。改动代码后请更新它;更早的进展见 `docs/LOG.md`。
## 任务目标

让 Windows 上双击 `dsh-launcher.exe` 一键打开 DSH(DeepSeek Harness)网页界面:静默启动
`npx @deepseek-ai/dsh web`、开干净独立 Chrome 窗口、关窗即停、无残留、不干扰别人的服务。
功能已完成并验证,处于维护状态。
## 测试命令

无自动化测试(无 `tests/`、无 `#[test]`)。回归 = 两套 `DSH_*` 端到端场景,共 16 项判据:

```powershell
cargo build --release
# A:首选端口空闲(不要设 DSH_SERVER_EXTRA,以验证 DSH_PORT 真透传)
$env:DSH_DATA_DIR="K:\tmp\dshA"; $env:DSH_PORT="3099"; $env:DSH_FAKE_CHROME="12"; $env:DSH_NO_UI="1"; .\target\release\dsh-launcher.exe
# B:先手起服务占住 3099(它就是"别人的服务"),再启动 launcher:应改用其它端口且不碰它
npx --yes @deepseek-ai/dsh web --no-open --port 3099
$env:DSH_DATA_DIR="K:\tmp\dshB"; .\target\release\dsh-launcher.exe
```

判据 A(8 项):`server ready` 落在 3099 且带 `?token=`、命令行含 `--port 3099`、该 URL 直连 `303`→follow
`200`、响应体 >20000、裸地址 `401`、关窗后打印 `DSH launcher exit` 且端口释放。判据 B(8 项):识别占用、
命令行含 `--port 0`、新端口 ≠3099、新地址 `303`/`200`、原服务在运行期间与 launcher 退出后都仍在监听且 URL 仍 `200`。
## 本次完成及验证结果

- 代码与文档已在 `c4601f4` 提交完毕(`src/main.rs` 三处修复 + README + AGENTS/CLAUDE/HANDOFF)。
- 验证(2026-09-11):A 场景 **8/8 PASS**、B 场景 **8/8 PASS**,用的是与 `c4601f4` 一致的源码与 exe。
- 本次存档会话重跑**被单实例锁阻塞,未能执行**(非代码失败):17:02 双击启动的实例(pid 26660)持有
  `Local\dsh-launcher-single`,两次启动都只写一行 `another launcher instance is running; exiting`。按"不临时修"
  原则如实记录,未结束任何进程、未改任何代码;该实例日志本身即新版正面证据(`--port 3080` + `server ready: …?token=…`)。
- 工作区干净、无新提交,故上次 16/16 的结果对本提交仍然有效。
## 下一步 TODO

1. 关掉正在运行的实例后重跑 A/B 两套场景,确认 16/16。
2. 排查旧日志里 09-10 出现的"打开浏览器后 1 秒即关闭"异常(与本轮问题无关)。
3. 若某版 dsh 不再打印带 token 的 URL,固定端口路径会在 5 秒宽限后回落裸地址(必 401);届时改为直接报错。
4. `DSH_FAKE_CHROME=N` 实际是 `ping -n 2N`(约 2N 秒),与 README "N 秒后关闭" 措辞不符,可择机校正。
5. 远端未推送:本地 `main` 领先 `origin/main`,`c4601f4` 尚未 `git push`。
## 当前的坑

- **裸地址必失败**:`dsh web` 的 UI 要进程级 launch token(只在其内存、不落盘、无开关可关),裸地址恒
  `401`(浏览器上可能显示成 404)。启动器从 `server.log` 解析它打印的那行 URL;日志追加写,必须只扫
  `log_from` 之后的新增字节,别整文件搜索,否则会复用上一次的旧 token。
- **单实例锁会静默退出**:有实例在跑时新实例不报错、不起服务、直接退出——跑测试前先确认没有 launcher 在运行。
- `DSH_FAKE_CHROME` 分支**不会**打 `opening … with chrome`,取地址要读 `server ready:` 行。
- 端口探测超时仅 400ms;`netstat` 里的 `TIME_WAIT` 不是 LISTENING,不参与判定。
## 下次恢复需打开的关键文件

`src/main.rs`(全部逻辑:`start_server`/`find_launch_url`/`url_port`/`wait_ready`/`run`)、`README.md`(行为与
`DSH_*` 开关表)、`AGENTS.md`(架构与约定)、`docs/LOG.md`(会话存档)。
## 最新 commit

```
c4601f4  修复:用 dsh 打印的 token URL 打开浏览器,并修正 DSH_PORT 与端口占用处理
316498c  Initial release: DSH one-click launcher for Windows  ← origin/main 仍停在这里,未 push
```

第 3 步无代码变更,故此处填上一次的代码提交;紧随其后的是一笔 `docs: 会话存档`。
