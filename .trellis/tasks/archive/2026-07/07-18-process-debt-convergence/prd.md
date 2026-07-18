# PRD: 仓库过程债专项收敛

## Goal

把仓库从“多条半开 WIP + 失真路线图 + 本地垃圾”收敛到可管理状态：  
**最多 0–1 条 Now**，paused/WIP 有明确 Keep-Later 台账，本地噪音下降，后续开工不再踩过程雷。

用户价值：降低认知负担，恢复“一次只做一件事”的执行节奏；**不改变**线上代理池业务语义。

## Background

- 用户选择方案 **2**：阶段 0 止血 + 阶段 1 过程债收敛（**不含** god-file 代码减肥）。
- 任务目录：`.trellis/tasks/07-18-process-debt-convergence`。

## Decisions

| # | 决策 | 选择 | 含义 |
|---|------|------|------|
| D1 | stash 默认处置 | **Keep-Later + 保留 stash** | 7 条均 Keep-Later；建台账；**禁止** drop/apply；不恢复功能代码 |
| D2 | ROADMAP Now | **执行期=本任务；完成后置空** | 实施时 Now=`process-debt-convergence`；归档后 Now 为空，下一条须从 Ready `start` |
| D3 | 本地清理 | **标准清理** | 过期 worktree + `temp/` + `.tmp_verify/` + `protoc.exe` + `cargo clean` + `proxy_ip_suggest.txt` 取消跟踪并 gitignore；不清理广义 `.claude` 缓存 |
| D4 | ROADMAP 改写深度 | **结构收敛** | 修 Now；Paused→Keep-Later 表；修 Later 幽灵指针；强化管理原则；压缩过时创建建议；Done 保留并标注历史 |
| D5 | Ready/Next | **不重排** | 仅修正失真/幽灵引用；不改产品优先级 |

## Confirmed Facts

### 路线图与任务

- 盘点时 `docs/ROADMAP.md` **Now** 仍写 `business-e2e-smoke-v1`，实际已归档  
  `.trellis/tasks/archive/2026-07/07-07-business-e2e-smoke-v1`（失真 Now）。
- Paused Closeout 6 项 + 额外 stash 1 项，见下表（索引以执行时 `git stash list` 为准，message 为稳定键）。
- Later 中多项已归档但仍写“任务目录存在”：  
  `proxy-quality-recommendations-dry-run`、`xray-config-dry-run-and-remove`、`warp-ops-enhancement`、`fetcher-validator-quality`。
- Ready：`metrics-low-cardinality-audit-v1`；Next：`api-readonly-contract-minimal-v1`（本任务不重排）。
- archive `2026-07/` 历史任务保留，不删除。

### Stash 台账种子（2026-07-18；全部 Keep-Later）

| message（稳定键） | 主要触点 | 规模 |
|------------------|----------|------|
| `wip: paused quality dashboard readonly` | web Dashboard/types | ~+181 |
| `wip: paused revalidation scheduler priority` | `scheduler.rs` | ~+207/-26 |
| `wip: paused fetcher source quality ranking` | core + tests + docs | ~+731 / 20 files |
| `wip: paused mcp api contract smoke v2` | integration tests + docs | ~+61 |
| `wip: paused dashboard ops polish v2` | web Dashboard/McpDebug | ~+324 |
| `wip: paused update failure hardening` | `proxy-mcp/lib.rs` | ~+121/-137 |
| `wip: paused fetcher circuit work` | fetcher/base + scheduler | ~+243 |

### 本地噪音

- `target/` ~47GB；过期 worktree 2 个（`2b45195`）；`protoc.exe`；`temp/`；空 `.tmp_verify/`；  
  被跟踪的 `proxy_ip_suggest.txt`（代码/配置无引用）。

## Requirements

### R1 — ROADMAP 校准（D2+D4+D5）

1. 执行期 Now = `process-debt-convergence`；删除 `business-e2e-smoke-v1` 为 Now 的表述。
2. 本任务完成收尾时 Now 置空，并写明：无业务 Now；下一条从 Ready 经 Trellis `start`。
3. Paused Closeout 长文 → Keep-Later 表（slug、优先级、stash message、一句话意图、恢复注意）。
4. 修正 Later 幽灵指针（改为 archive 路径或“历史/仅 stash”）。
5. 强化管理原则（见 R4）；压缩过时「任务创建建议」长列表。
6. Done 保留，标注为历史完成、非当前主线。
7. Ready/Next/Parking **不重排**（D5）。

### R2 — 处置台账（D1）

1. 产出 `.trellis/tasks/07-18-process-debt-convergence/inventory.md`，覆盖 7 条 stash + ROADMAP 相关 paused/Later 草稿。
2. 当前行一律 **Keep-Later**；枚举字段可含 Resume/Kill 供日后改判。
3. 恢复说明：以 `git stash list` + message 定位；**本任务不 apply**。
4. ROADMAP 表与 inventory 字段对齐，避免双源矛盾。

### R3 — 本地清理（D3）

1. `git worktree remove` 两个过期 worktree（先确认无独有未提交工作）。
2. 删除 `temp/`、`.tmp_verify/`、`protoc.exe`（存在才删）。
3. `cargo clean`。
4. `git rm --cached proxy_ip_suggest.txt`（或等价）并更新 `.gitignore`。
5. 禁止 stash drop/clear/apply；禁止删 archive。

### R4 — 过程协议

写入 ROADMAP 管理原则（不另起长文除非必要）：

1. 同时最多 1 个 in_progress / Now。
2. 新想法进 Parking Lot，不直接堆半截 stash。
3. 暂停：ROADMAP 一行 + 可选 `wip: paused <slug>` stash + 清空 current task。
4. 完成：归档任务 + 更新 ROADMAP Now/Done。

## Acceptance Criteria

- [ ] **AC1** Now 与 Trellis/归档一致；执行期为本任务，收尾后为空；无已归档任务冒充 Now。
- [ ] **AC2** `inventory.md` 覆盖 7 stash + 相关 paused/Later；处置均为 Keep-Later（或显式改判记录）。
- [ ] **AC3** ROADMAP 无幽灵 active-task 路径；stash 声明与 `git stash list` message 可对上。
- [ ] **AC4** 两过期 worktree 已移除；`temp/`、`.tmp_verify/`、`protoc.exe` 已清理。
- [ ] **AC5** `cargo clean` 后 `target/` 体积显著下降（本任务默认必须执行 clean）。
- [ ] **AC6** `git status` 仅有意变更：`docs/ROADMAP.md`、`.gitignore`、取消跟踪 `proxy_ip_suggest.txt`、任务目录台账/规划；**无** paused 功能代码恢复。
- [ ] **AC7** ROADMAP 管理原则含 1-WIP 与暂停/完成仪式。
- [ ] **AC8** 无业务功能实现提交；可选 commit 为 `chore`/`docs` 过程治理。
- [ ] **AC9** Ready/Next 相对顺序与本任务开始前一致（D5），仅允许修错误指针/措辞。

## Out of Scope

- 恢复/实现任一 paused 功能；stash apply/drop
- god-file 拆分、crate 重构
- 删除或重写 archive 历史任务正文
- 部署、`update_service`、SSH、DeepWiki
- 产品级 Ready/Next/Later 全量重排

## Open Questions

无阻塞项。D1–D5 已决。

## Notes

- 执行前需规划文档审阅通过，再 `task.py start`。
- stash 索引号会变；**message 为稳定键**。
