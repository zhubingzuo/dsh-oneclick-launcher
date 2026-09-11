# HANDOFF

> 给接手的人(或下一个会话)。改动代码后请回来更新本文件。

## 任务目标

本轮三件事,均已做完并验证:

1. 修复"双击后 DSH 页面打不开"(Chrome 停在错误页)。
2. 修掉顺带发现的两个问题:`DSH_PORT` 开关失效;首选端口被占用时必然失败。
3. 补齐仓库文档(`AGENTS.md` / `CLAUDE.md` / `HANDOFF.md`)并同步 `README.md`。

## 测试命令

没有自动化测试(无 `tests/`,无 `#[test]`)。回归靠 `DSH_*` 开关做端到端,分两个场景。

### 场景 A:首选端口空闲

```powershell
cargo build --release
$env:DSH_DATA_DIR="K:\tmp\dsh-verify\A"; $env:DSH_PORT="3099"; $env:DSH_FAKE_CHROME="12"; $env:DSH_NO_UI="1"
# 不要设置 DSH_SERVER_EXTRA,这样才能验证 DSH_PORT 真的透传给了 dsh
.\target\release\dsh-launcher.exe
```

### 场景 B:首选端口已被占用

```powershell
# 先手起一个服务占住 3099,它就是"别人的服务"
npx --yes @deepseek-ai/dsh web --no-open --port 3099

# 再启动 launcher:它应当改用系统分配的空闲端口,且不碰上面那个
$env:DSH_DATA_DIR="K:\tmp\dsh-verify\B"; $env:DSH_PORT="3099"; $env:DSH_FAKE_CHROME="12"; $env:DSH_NO_UI="1"
.\target\release\dsh-launcher.exe
```

断言与 2026-09-11 的实测结果(全部通过):

| 场景 | # | 断言 | 期望 | 实测 |
| --- | --- | --- | --- | --- |
| A | A1 | `launcher.log` 的 `server ready:` 地址 | 落在 3099 且带 `?token=` | PASS |
| A | A2 | 启动命令行 | 含 `--port 3099` | PASS |
| A | A3 | `curl.exe -s -o nul -w "%{http_code}" "<该 URL>"` | `303` | `303` |
| A | A4 | 同 A3 再加 `-L -c ck.txt` | `200` | `200` |
| A | A5 | 响应体字节数 | > 20000 | 27660 |
| A | A6 | 对照组:裸地址 `http://127.0.0.1:3099/` | `401` | `401` |
| A | A7 | 关窗后 | 打印 `DSH launcher exit`、端口释放 | PASS |
| B | B1 | 识别占用 | `port 3099 already in use: true` | PASS |
| B | B2 | 端口策略 | 命令行含 `--port 0` | PASS |
| B | B3 | 新端口 | ≠ 3099(实测 13593) | PASS |
| B | B4 | 新地址直连 / follow | `303` / `200` | `303` / `200` |
| B | B6 | launcher 运行期间 | 原服务仍在监听 3099 | PASS |
| B | B7 | 原服务的 token URL | 仍 `200` | `200` |
| B | B8/B9 | launcher 退出后 | 原服务仍在监听,URL 仍 `200` | PASS |

坑与注意:

- `DSH_FAKE_CHROME=N` 实际执行的是 `ping -n 2N`,即约 `2N` 秒后"关窗";该分支**不会**打
  `opening … with chrome` 那行日志,要取地址请读 `server ready:` 行。
- 真实 Chrome 那条路径无法自动断言"窗口关闭",需要人工双击确认一次:2026-09-11 16:48 的实机运行
  已确认日志里 `opening http://127.0.0.1:3080/?token=…` 正确。
- 端口探测超时只有 400ms;`netstat` 输出里的 `TIME_WAIT` 不是 LISTENING,不影响判定。

## 当前状态

- 三项修复与文档已完成并验证,**尚未提交**(见文末「最新 commit」)。
- 改动明细:
  1. **token(本次核心)**:`start_server` 额外返回 `log_from`(spawn 前的日志长度);新增
     `find_launch_url(from_offset)`(不再限定端口)与 `url_port()`;`wait_ready` 以"已打印带 token
     的 URL 且该端口可连"为就绪条件(仅固定端口时有 5 秒宽限,超时回落裸地址作兜底);`run()` 用带
     token 的地址开浏览器。
  2. **`DSH_PORT`**:默认命令里显式追加 `--port <DSH_PORT>`,该变量从此真正生效(此前只影响探测,
     不影响 dsh 服务端口)。
  3. **端口被占**:首选端口被占用时改用 `--port 0` 另起自己的 dsh web;`run()` 原先的
     `if !already_running { … } else { 只开浏览器 }` 已合并成单一"总是启动自己的服务"路径,退出时
     无条件结束自己的进程树(别人的服务不碰)。
  4. `JobObjectExtendedLimitInformation` 补 `job_memory_limit` 字段(对齐 winnt.h 布局);日志文案
     `cmd /C …` → `cmd …`。
  5. 文档:`AGENTS.md`、`CLAUDE.md`(内容只有一行 `@AGENTS.md`)、本文件;`README.md` 同步端口行为。
- 可双击副本:根目录 `dsh-launcher.exe` 已刷新为本次构建,sha256
  `624279638945aae616bc69f7f847a5dec3500b4fdb8dfe569c1a3b160c557760`(与
  `target\release\dsh-launcher.exe` 一致)。

### 背景:为什么裸地址一定失败(别急着怀疑端口)

`dsh web`(实测版本 `0.1.5-rc.1`)对 UI 有**进程级鉴权**:

- 每次启动生成 `randomBytes(32)` 的 launch token,仅存于该 node 进程内存(WeakMap),**不落盘**;
- `dsh web --help` 没有关闭鉴权的开关,包内也没有相关环境变量(只有 `DSH_HOME` /
  `DSH_LAUNCH_ENVIRONMENT_KEY` / `DSH_TELEMETRY_DISABLED`);
- 裸 `GET /` → **401** `dsh web authentication required; reopen the URL printed by dsh web.`
  (浏览器上可能被显示成 404 错误页);
- `GET /?token=<token>` → **303** 并种下 HttpOnly cookie `dsh-auth-<sha256(authority)>`,之后才 200;
- 唯一能拿到 token 的位置,就是它启动时打在 **stdout** 的那行
  `dsh web: http://127.0.0.1:<port>/?token=<...>`。

启动器原先把这行连同 stderr 一起重定向进 `server.log` 后就再没读过,始终只开裸地址;而每次运行都用
全新的临时 Chrome profile(用完即删),cookie 从不复用 → **必然失败**。此外旧版在端口被占时还会走
"不抢已有服务,只开浏览器"分支,拿不到 token 却照样开裸地址,于是次次失败——这就是"老是打不开"的
由来。现在前者改为解析 stdout 的带 token 地址,后者改为另起一个自己的服务。

## 下一步 TODO

1. 旧日志里 09-10 出现过"打开浏览器后 1 秒即关闭"的异常(与本次问题无关,未排查)。
2. `launcher.log` / `server.log` 都是追加写的:排查时注意区分不同构建留下的历史行(09-10 曾有带
   token 处理的构建;旧版命令行里也没有 `--port`)。
3. 若将来某个 dsh 版本不再打印带 token 的 URL,固定端口路径下 `wait_ready` 会在 5 秒宽限后回落裸
   地址(那时会 401)。真出现时应改为直接报错而不是开一个注定失败的地址。
4. `DSH_FAKE_CHROME` 的实际语义是"秒数 × 2"(见上文坑),与 README 里"打开 N 秒后关闭"的措辞
   不符,可择机校正。
5. 本轮只处理了「首选端口被占」这一种冲突,没有做"提示用户并可选择结束占位进程"的交互(当前策略是
   回避而非清理),若确有此需求再议。

## 最新 commit

```
上一笔(本次修复之前):316498c6b495ec67e562ebdafc58733c38d5f653  2026-09-09 11:55
                      Initial release: DSH one-click launcher for Windows
分支 main(跟踪 origin/main;origin = https://github.com/zhubingzuo/dsh-oneclick-launcher.git)
```

本次修复与本文档**同属一笔提交**,所以这里无法写入它自身的 hash(自引用)。查看当前 HEAD:

```powershell
git log -1 --format='%h %s'
```

该提交信息以「修复:用 dsh 打印的 token URL 打开浏览器」开头。
