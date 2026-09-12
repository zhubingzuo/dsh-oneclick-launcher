# 会话存档

- 2026-09-11 | c4601f4 | 修复 dsh 进程级鉴权导致的"页面打不开"(改为解析其 stdout 打印的 token URL);DSH_PORT 真正透传为 `--port`;首选端口被占改用 `--port 0` 另起自己的服务且不碰原有服务;补 AGENTS.md/CLAUDE.md/HANDOFF.md 与 README 同步
- 2026-09-12 | f03321e | 抗 DSH 版本变化:新增外部配置 `dsh-launcher.conf`(命令/参数/端口/URL 标记/超时)、首选端口上"无令牌服务直接复用"、启动参数回退链(完整→去 --port→仅命令)、URL 双形态解析且裸地址仅在 200/303 时使用、新增 `DSH_SKIP_SINGLE_INSTANCE` 与 `scripts/regression.ps1`(A/B/C 20 项判据,两次 20/20 PASS);版本 1.1.0
- 2026-09-12 | (docs) | 仓库改为中英双语:新增英文 `README.en.md`,两份 README 顶部互相链接并加 Release/License 徽章;GitHub About 描述、topics、v1.1.0 Release 说明同步改为中英双语
