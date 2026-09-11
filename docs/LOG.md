# 会话存档

- 2026-09-11 | c4601f4 | 修复 dsh 进程级鉴权导致的"页面打不开"(改为解析其 stdout 打印的 token URL);DSH_PORT 真正透传为 `--port`;首选端口被占改用 `--port 0` 另起自己的服务且不碰原有服务;补 AGENTS.md/CLAUDE.md/HANDOFF.md 与 README 同步
