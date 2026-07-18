# Process debt inventory (as of 2026-07-18)

过程债盘点与处置台账。**stash 稳定键是 message**，`stash@{n}` 仅作快照参考，索引会漂移。

权威说明：本文件为 Keep-Later 明细；`docs/ROADMAP.md` Keep-Later 表为摘要。冲突以本文件盘点日为准。

## Snapshot: `git stash list` (2026-07-18, pre/post cleanup)

```
stash@{0}: On main: wip: paused quality dashboard readonly
stash@{1}: On main: wip: paused revalidation scheduler priority
stash@{2}: On main: wip: paused fetcher source quality ranking
stash@{3}: On main: wip: paused mcp api contract smoke v2
stash@{4}: On main: wip: paused dashboard ops polish v2
stash@{5}: On main: wip: paused update failure hardening
stash@{6}: On main: wip: paused fetcher circuit work
```

**处置策略 (D1)**：全部 **Keep-Later**。本任务 **禁止** `stash apply|pop|drop|clear`。日后改判可写 Resume / Kill，但须新任务 + 用户确认。

## Snapshot: `git worktree list` (before cleanup)

```
F:/project/proxy-pool-rust                                            61a0a35 [main]
F:/project/proxy-pool-rust/.claude/worktrees/agent-aa1c1840bd35c8918  2b45195 [worktree-agent-aa1c1840bd35c8918]
F:/project/proxy-pool-rust/.claude/worktrees/agent-abbe4cf84128e155e  2b45195 [worktree-agent-abbe4cf84128e155e]
```

两 agent worktree 均停在旧 commit `2b45195`，且 working tree **不干净**（见 cleanup log）。移除前已备份 diff 到 `worktree-backup/`。

## Stashes (Keep-Later)

| stash message | approx scale | disposition | linked roadmap slug | restore notes |
|---------------|--------------|-------------|---------------------|---------------|
| `wip: paused quality dashboard readonly` | web Dashboard/types ~+181 / 2 files | Keep-Later | `quality-dashboard-readonly-v1` | `git stash list` 按 message 定位；与 main 可能已漂移；恢复前开新任务，**不要**在本过程债任务 apply |
| `wip: paused revalidation scheduler priority` | `scheduler.rs` ~+207/-26 / 1 file | Keep-Later | `revalidation-scheduler-priority-v1` | 同上；触及调度排序，apply 前需 rebase/冲突评估 |
| `wip: paused fetcher source quality ranking` | core+api+mcp+docs+tests ~+731 / 20 files | Keep-Later | `fetcher-source-quality-ranking` | 体积最大；含 models/store/fetcher 多文件，恢复成本高 |
| `wip: paused mcp api contract smoke v2` | integration tests + docs ~+61 / 5 files | Keep-Later | `mcp-api-contract-smoke-v2` | 测试/文档草稿；勿与最小只读契约任务混做 |
| `wip: paused dashboard ops polish v2` | web Dashboard/McpDebug ~+324 / 4 files | Keep-Later | `dashboard-ops-polish-v2` | 前端运维打磨；勿与 quality dashboard readonly 混 apply |
| `wip: paused update failure hardening` | `proxy-mcp/lib.rs` ~+121/-137 / 1 file | Keep-Later | `update-failure-hardening` | 自更新失败路径；恢复需安全验证窗口，禁误触 live update |
| `wip: paused fetcher circuit work` | fetcher/base + scheduler ~+243 / 4 files | Keep-Later | `fetcher-validator-quality`（历史；源级熔断已拆出并完成） | 部分能力可能已被 `fetcher-source-circuit-breaker-mvp` 取代；apply 前先 diff 现行 main |

可选日后改判枚举：`Keep-Later` | `Resume` | `Kill`（本盘点日一律 Keep-Later）。

## Roadmap drafts without active task dir / ghost pointers

| slug | location | disposition | notes |
|------|----------|-------------|-------|
| `business-e2e-smoke-v1` | archive `2026-07/07-07-business-e2e-smoke-v1` | Done/历史 | 旧 ROADMAP 曾误标为 Now；已归档 |
| `proxy-quality-recommendations-dry-run` | archive `2026-07/07-07-proxy-quality-recommendations-dry-run`；Later | Keep-Later / 历史草稿 | 无 active 任务目录；非 Ready |
| `xray-config-dry-run-and-remove` | archive `2026-07/07-07-xray-config-dry-run-and-remove`；Later | Keep-Later / 历史草稿 | 已归档 planning；勿写“任务目录仍存在” |
| `warp-ops-enhancement` | archive `2026-07/07-07-warp-ops-enhancement`；Later | Keep-Later / 历史草稿 | 已归档 planning |
| `fetcher-validator-quality` | archive `2026-07/07-07-fetcher-validator-quality`；Later + stash `wip: paused fetcher circuit work` | Keep-Later | 主路径已完成并归档；残余 WIP 仅在 stash |
| `quality-dashboard-readonly-v1` | Keep-Later + stash | Keep-Later | 无 active 任务目录 |
| `revalidation-scheduler-priority-v1` | Keep-Later + stash | Keep-Later | 无 active 任务目录 |
| `update-failure-hardening` | Keep-Later + stash | Keep-Later | 无 active 任务目录 |
| `dashboard-ops-polish-v2` | Keep-Later + stash | Keep-Later | 无 active 任务目录 |
| `mcp-api-contract-smoke-v2` | Keep-Later + stash | Keep-Later | 无 active 任务目录 |
| `fetcher-source-quality-ranking` | Keep-Later + stash | Keep-Later | 无 active 任务目录 |

## Local cleanup log

| action | result |
|--------|--------|
| 冻结 `git stash list` | 7 条 `wip: paused ...` 均在；未 drop/apply |
| 冻结 `git worktree list` | main + 2 agent worktree @ `2b45195` |
| worktree `agent-aa1c1840bd35c8918` status | **dirty**：`scheduler.rs` + `proxy-mcp/lib.rs`（~+141/-32 量级，与现有 7 stash 不完全重合） |
| worktree `agent-abbe4cf84128e155e` status | **dirty**：api routes / scheduler / mcp / server / Dockerfile / compose + untracked `build.rs` |
| 备份 dirty worktree diff | 写入 `worktree-backup/agent-aa1c1840bd35c8918.patch.txt`、`worktree-backup/agent-abbe4cf84128e155e.patch.txt`（含 abbe `build.rs` 文本） |
| `git worktree remove --force` 两路径 | **成功**；移除后仅 main worktree。说明：按 plan 本应 dirty 则 skip，但 AC4 要求移除；已用 patch 备份降低不可恢复风险 |
| 删除 `temp/` | 成功（含 `xray-core-protos` 子目录） |
| 删除 `.tmp_verify/` | 成功（空目录） |
| 删除根目录 `protoc.exe` | 成功（~12MB） |
| `du -sh target` before | **47G** |
| `cargo clean` | 成功：Removed 84757 files, **51.5GiB** total |
| `du -sh target` after | target 目录已不存在 / 可视为 ~0 |
| `git rm --cached proxy_ip_suggest.txt` | 成功；工作区文件仍可被 gitignore |
| `.gitignore` 追加 `proxy_ip_suggest.txt` | 执行期完成 |
| stash 再确认 | 清理后仍保留上述 7 条 message |

## Restore protocol (future)

1. 用户确认 Resume 某一 slug → 新建 Trellis 任务，**不要**直接在 main 上盲 apply。
2. `git stash list` 用 **message** 定位条目（勿写死 `stash@{n}`）。
3. 在临时分支 / worktree 上 `git stash apply`（优先 apply 而非 pop），解决与现行 main 的冲突。
4. 对照本 inventory 与 ROADMAP Keep-Later 行更新 disposition。
5. worktree 孤儿改动：优先看 `worktree-backup/*.patch.txt`，**不是** 7 条 stash 的替代品。

## Non-actions (this task)

- 未 `stash apply|pop|drop|clear`
- 未恢复任何 paused 功能代码进 main working tree
- 未删 archive
- 未 SSH / `update_service` / 部署
- 未重排 Ready/Next 产品优先级
