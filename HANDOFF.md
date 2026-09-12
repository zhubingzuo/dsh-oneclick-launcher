# HANDOFF

> 接手前先读本文件。改动代码后请更新它;更早的进展见 `docs/LOG.md`。
## 任务目标

让 Windows 上双击 `dsh-launcher.exe` 一键打开 DSH(DeepSeek Harness)网页界面:静默启动
dsh web、开干净独立 Chrome 窗口、关窗即停、无残留、不干扰别人的服务;并且 **DSH 升级后通常
不需要重新编译本启动器**(地址取自 dsh 自身输出,启动方式写在外部配置文件里)。功能已完成并验证。
## 测试命令

无单元测试,回归 = `scripts/regression.ps1`(A/B/C 三套、20 项判据,约 2 分钟):

```powershell
cargo build --release
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/regression.ps1
```

- **A** 首选端口(3099)空闲 → 启动器应自己起服务(`--port 3099`)、读到带 token 的 URL、
  关窗后打印 `stopping server tree` 且端口释放。
- **B** 3099 已被**带 token**的 dsh 占用 → 应识别 `HTTP 401`、改用 `--port 0` 另起实例,
  原服务在 launcher 运行期间与退出后都仍在(仍 401)。
- **C** 3099 已被**无 token**的 stub 服务占用 → 应直接复用(日志出现
  `already serves the UI without a token; reusing it`)、**不出现** `server command:`、
  退出时**不出现** `stopping server tree`,且 stub 仍存活。

脚本会设置 `DSH_SKIP_SINGLE_INSTANCE=1`,因此**本机正在使用的启动器实例无需关闭**;它只用
`.testdata\` 下的独立数据目录与 3099 端口。跑之前先确认 3099 空闲(脚本会检查)。
## 本次完成及验证结果

- 代码与文档提交在 `f03321e`(8 文件,+746/-182):核心是"抗版本变化"改造,详见下条。
- 验证(2026-09-12):先用临时脚本、再用仓库内的 `scripts/regression.ps1` 各跑一次,
  **两次均 20/20 PASS**(A 7 项、B 7 项、C 6 项)。
- 期间**没有**结束任何用户正在使用的进程:本次会话所依赖的 dsh web 与它的 launcher
  (pid 28400,启动的 3080 服务)全程保持运行,仅测试自己的实例被启动/结束。
## 本次改动要点(抗 DSH 版本变化)

1. **外部配置 `dsh-launcher.conf`**:exe 同目录首次运行自动生成(不可写时退回数据目录),
   含 `command` / `extra_args` / `port` / `url_marker` / `timeout_secs`;`DSH_LAUNCHER_CONFIG`
   可指定路径。DSH 改动启动方式时改文本即可,无需重编译。
2. **复用而非抢占**:首选端口上的服务若对裸地址返回 200/303(老版本、无需令牌)则**直接复用**,
   关窗时不动它;若 401(需令牌)或非 HTTP,则用 `--port 0` 另起自己的实例。
3. **启动回退链**:`完整命令 → 去掉 --port → 仅命令`,任何一次在就绪前退出就换下一档,
   因此参数被改名/移除时仍能启动。
4. **URL 解析双形态**:先按 `url_marker` 取标记后第一个 http(s) 地址;失败再全局匹配
   "带 token 的本地地址"。裸地址**仅在真能 200/303 时**才被交给浏览器,否则弹窗提示改
   `url_marker`,不再出现"静默打开 401 页面"。
5. **测试基建**:新增 `scripts/regression.ps1` + `scripts/test-stub-server.ps1`,以及
   `DSH_SKIP_SINGLE_INSTANCE` 开关(解决"实例在跑就没法回归"的老问题)。
## 下一步 TODO

1. 关掉正在运行的实例后,**手动**把 `target\release\dsh-launcher.exe` 覆盖到仓库根目录的
   便捷副本 `dsh-launcher.exe`(该副本当前被运行中的实例锁定,仍是 1.0 时代的旧版)。
2. 已推送 `f03321e`/`643154f` 到 `origin/main`(本地与远端一致),并已建 Release
   `v1.1.0`(附件 `dsh-launcher.exe`,308224 字节)。后续发新版时:改 `Cargo.toml` 版本 →
   构建 → 提交推送 → 用同样的方式建 tag/Release。
3. 若某版 dsh 既不打印带 token 的 URL、裸地址又不可用,现在会明确报错并指向 `url_marker`
   (旧行为是"5 秒宽限后回落裸地址"),这是有意为之,不要改回静默回落。
4. 可选的下一步演进:把"复用无令牌服务"的判断从 `GET /` 扩展到更明确的能力探测;
   以及给 `.conf` 增加"额外尝试命令列表"(目前回退链是内置三档)。
## 当前的坑

- **单实例锁会静默退出**:有实例在跑时新实例只写一行日志就退出;跑回归务必带
  `DSH_SKIP_SINGLE_INSTANCE=1`。
- **裸地址必失败**:`dsh web` 的 UI 要进程级 launch token(只在其内存、不落盘、无开关可关),
  裸地址恒 `401`。启动器只把 dsh 打印的带 token URL 交给浏览器。
- **日志追加写**:解析 dsh 输出必须只用 `log_from` 之后的新增字节,否则会复用上一次的旧 token。
- **端口探测超时仅 400ms**;`netstat` 里的 `TIME_WAIT` 不是 LISTENING,不参与判定。
- **仓库根目录的便捷 exe 会被运行中的实例锁定**,无法覆盖;必须先关窗口。
- `DSH_FAKE_CHROME=N` 实际是 `ping -n 2N`(约 2N 秒)。README 已按"约 N 秒"描述。
## 下次恢复需打开的关键文件

`src/main.rs`(全部逻辑:`LaunchConfig::load`/`config_path`/`find_launch_url`/`http_status`/
`build_attempts`/`start_server`/`wait_ready`/`run`)、`README.md`(行为表与 `DSH_*` 开关)、
`AGENTS.md`(架构与约定)、`scripts/regression.ps1`(回归)、`docs/LOG.md`(会话存档)。
## 最新 commit

```
f03321e  feat: 抗 DSH 版本变化的启动方式(外部配置 + 复用已有服务 + 参数回退)   ← 本次代码提交
8eb0b9d  docs: 会话存档
c4601f4  修复:用 dsh 打印的 token URL 打开浏览器,并修正 DSH_PORT 与端口占用处理
316498c  Initial release: DSH one-click launcher for Windows
```
`origin/main` 已与本地一致(最新 `643154f`);GitHub Release:
<https://github.com/zhubingzuo/dsh-oneclick-launcher/releases/tag/v1.1.0>
