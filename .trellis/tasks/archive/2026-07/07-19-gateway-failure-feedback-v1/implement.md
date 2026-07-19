# Implement: gateway-failure-feedback-v1

## Preconditions

- [x] PRD filled; D1=B confirmed
- [x] design.md written
- [ ] User reviewed artifacts → `task.py start`（**未 start 前禁止业务代码**）
- [ ] Now 仅本任务

## Checklist

### 1. Store cooldown helpers

- [ ] 在 `proxy-core` 增加 gateway cooldown Redis API（`store.rs` 方法或 `gateway_cooldown.rs` + store 包装）
- [ ] Key：`gateway:cooldown:proxy:{dedup_key}`、`gateway:cooldown:xray:{port}`
- [ ] TTL = 300（与 `POOL_PROXY_FAILURE_COOLDOWN` / `XRAY_FAILURE_COOLDOWN` 同源常量，避免双源数字）
- [ ] put / clear / is_cooling；错误向上返回，调用方 fail-open

### 2. Wire `record_upstream_attempt`

- [ ] Proxy/Xray Failure：进程内 map + best-effort put Redis
- [ ] Proxy/Xray Success：清 map + best-effort clear Redis
- [ ] Warp / Direct / WarpChain / NoProxy 行为不变

### 3. Wire selection filters

- [ ] `try_pool_candidates`：process OR redis cooldown → skip
- [ ] `try_xray`：同上
- [ ] Redis 读失败 → 不 skip（fail-open）

### 4. Tests

- [ ] free_pool：Failure 后同窗口不再优先选中（AC1）
- [ ] xray：Failure 后不再优先（AC2）
- [ ] Success 清除后可再选（AC3）
- [ ] **仅 Redis**（无 process map）仍跳过（AC4 短重启代理）
- [ ] 既有 process-map pure tests 仍绿

### 5. Docs

- [ ] `.trellis/spec/proxy-core/backend/xray-route-eligibility.md` 补充 Redis TTL 层
- [ ] 按需 `quality-guidelines` / index 一行
- [ ] ROADMAP P0-B 验收勾选（完成归档时）

### 6. Validate

```bash
cargo test -p proxy-core --lib route_debug::
cargo test -p proxy-core --lib store::
cargo clippy -p proxy-core --lib -- -D warnings
cargo fmt
```

（若 helpers 在独立模块，补对应 filter。）

### 7. Finish

- [ ] trellis-check / 自检 AC
- [ ] 更新 ROADMAP Now→Done 交接
- [ ] commit（conventional）；**push 仅用户要求**
- [ ] `task.py archive` 流程

## Out of order / do not

- 不改 gateway `http_connect`/`socks5` 循环结构（已接线）
- 不调用 `mark_failed_with_circuit`
- 不 Resume stash
- 不新增 MCP/API
- 不扩 P0-C

## Rollback points

| After | Rollback |
|-------|----------|
| helpers only | delete helpers |
| wired record/select | revert route_debug + store |
| shipped | redeploy previous; keys expire 300s |
