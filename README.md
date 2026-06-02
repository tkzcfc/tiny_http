# tiny-http

## 测试工具

### 生成本地测试数据库

直接写入 SQLite，用于快速准备后台测试数据。

```bash
python tools/seed_test_data.py
```

常用参数：

```bash
python tools/seed_test_data.py --db data.db --reset-db --error-logs 1200 --client-stats 5000 --days 90
```

参数：

- `--db`：数据库路径，默认 `data.db`
- `--reset-db`：生成前删除当前数据库、`-wal`、`-shm`
- `--no-backup`：配合 `--reset-db` 使用，不备份旧数据库
- `--error-logs`：生成错误上报数量，默认 `1200`
- `--client-stats`：生成客户端首次上报数量，默认 `5000`
- `--days`：数据分布天数，默认 `90`
- `--seed`：随机种子，默认 `20260601`

生成后启动服务：

```bash
cargo run -- --admin-account fc --admin-password fc
```

### 通过接口上报测试数据

调用运行中的服务，测试 `/api/upload_log` 和 `/api/upload_statistics_cli_cfg`。

```bash
python tools/report_api_test.py
```

常用参数：

```bash
python tools/report_api_test.py --base-url http://127.0.0.1:8000 --error-logs 1000 --device-reports 1000 --concurrency 16
```

参数：

- `--base-url`：服务地址，默认 `http://127.0.0.1:8000`
- `--error-logs`：调用 `/api/upload_log` 次数，默认 `100`
- `--device-reports`：调用 `/api/upload_statistics_cli_cfg` 次数，默认 `100`
- `--concurrency`：并发请求数，默认 `8`
- `--timeout`：单个请求超时时间，默认 `8.0` 秒
- `--seed`：随机种子，默认 `20260601`
- `--duplicate-ratio`：重复错误比例，默认 `0.75`
- `--verbose`：打印每个请求结果
