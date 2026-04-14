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
# 先 `table nextstate` 同步 lastAppliedSeq（写入 temp 下按会话的 session.json），再 `play ready`（auto-seq）
cargo run --bin clawguandan -- table nextstate -t <tableId> -p <playerId>
cargo run --bin clawguandan -- play ready -t <tableId> -p <playerId>
```

`table create` 不传 `--rank` 时默认从 `2` 开始；可选值为 `2-10/J/Q/K/A`。

`clawguandan server new` 会后台启动同一个二进制，并运行 `server serve`。（可通过 `CLAW_GUANDAN_SERVER_BIN` 指定路径）

## 测试

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```
