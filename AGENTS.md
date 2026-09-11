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
| `src/main.rs` | 全部程序逻辑(单文件):单实例、Job Object、启动 dsh、就绪等待、开浏览器、关窗清理 |
| `build.rs` | 零依赖嵌图标(调用 `rc.exe`),失败静默降级 |
| `scripts/gen_icon.ps1` | 由 DSH 官方 `favicon.svg` 重新生成 `assets/icon.ico`(16–256 共 9 个尺寸) |
| `assets/icon.ico` | 程序图标 |
| `README.md` | 面向用户的功能说明、行为表、故障排查、内部测试开关列表 |

`src/main.rs` 主流程:`SingleInstance`(命名互斥量) → 探测 `DSH_PORT`(默认 3080)是否被占 →
`KillJob` + 启动 `cmd /C npx --yes @deepseek-ai/dsh web --no-open --port <port>`(端口被占时改用
`--port 0` 让系统分配) → `wait_ready`(等到它打印出带 token 的 URL 且该端口可连) → 用独立临时
profile 开 Chrome → 等窗口关闭 → `taskkill /T` + Job Object(`KILL_ON_JOB_CLOSE`)兜底 → 删临时
profile → 退出。

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

**本项目没有自动化测试**:`tests/` 不存在,`src/main.rs` 内也没有 `#[test]`。验证靠 `DSH_*`
环境变量手动作端到端回归,具体命令与断言清单见 `HANDOFF.md`。

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
- **不重用别人的服务、也不杀它**:`dsh web` 的访问令牌只在那一个进程的内存里,别人启的实例我们
  无法驱动(裸地址必 401)。端口被占时一律改用 `--port 0` 另起自己的服务,只结束自己启动的那个。
- 注释与标识符用英文,面向用户的文案用中文。
- 保持 `src/main.rs` 现有写法:单文件、`fn` 按主流程顺序排布、少抽象。

## 相关文档

- `HANDOFF.md`:当前任务、测试命令、状态、TODO、最新 commit —— 接手/新会话先读它。
- `README.md`:用户视角的功能与排障(含 `DSH_*` 测试开关表)。
