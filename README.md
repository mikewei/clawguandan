# clawguandan

MVP 实现见 [doc/design.md](doc/design.md)：Axum HTTP API + `clawguandan` CLI，支持建桌、加入、准备、`nextstate` 长轮询与 `seq` 乐观锁。

## 运行服务端

```bash
cargo run --bin clawguandan -- server serve
# 默认 0.0.0.0:22222；可用 `--ip <ip>`、`--port <port>` 或环境变量 `PORT` 覆盖
```

## CLI

```bash
cargo run --bin clawguandan -- server use 127.0.0.1:22222
cargo run --bin clawguandan -- server use 127.0.0.1:22222
cargo run --bin clawguandan -- table create "Friday"
cargo run --bin clawguandan -- table create "Friday" --rank 8
cargo run --bin clawguandan -- table join -t <tableId> --name Alice --seat auto
cargo run --bin clawguandan -- table join -t <tableId> --name Bot-S --type ai --model gpt-4o --seat auto
# join 会下发 playerKey，CLI 自动写入同会话目录下的 auth.json（无需手填）
# 先 `table nextstate` 同步 lastAppliedSeq（写入 session.json），再 `play ready`（auto-seq + auto playerKey）
cargo run --bin clawguandan -- table nextstate -t <tableId> -p <playerId>
cargo run --bin clawguandan -- play ready -t <tableId> -p <playerId>
# 如需显式覆盖，也可传 --player-key/-k
cargo run --bin clawguandan -- play ready -t <tableId> -p <playerId> -k <playerKey>
```

`table create` 不传 `--rank` 时默认从 `2` 开始；可选值为 `2-10/J/Q/K/A`。
`table join --model` 仅在 `--type ai` 时生效；非 AI 类型会静默忽略该字段。

`clawguandan server new` 会后台启动同一个二进制，并运行 `server serve`。（可通过 `CLAW_GUANDAN_SERVER_BIN` 指定路径）

## 规则（精简版 Markdown）

- 正文随二进制嵌入在 [`web/rules/`](web/rules/)（英文 `rules_en.md`、中文 `rules_zh.md`）；完整叙述仍以 [`doc/guandan_rules_en.md`](doc/guandan_rules_en.md) / [`doc/guandan_rules_zh.md`](doc/guandan_rules_zh.md) 为准。
- **HTTP**：`GET /api/v1/rules`（默认 `lang=en`），或 `GET /api/v1/rules?lang=zh`。响应为 Markdown，`Content-Type: text/markdown; charset=utf-8`。
- **CLI**（无需已配置 server）：`clawguandan show rules`、`clawguandan show rules --lang zh`。

## 测试

```bash
cargo test
cargo test --features test-utils   # 含 HTTP 集成测试（如 rules、tables API）
cargo clippy --all-targets -- -D warnings
```
