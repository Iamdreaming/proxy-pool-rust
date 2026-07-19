# Implement: 可用性优先收敛

## 0. Gate — 1-WIP（必须先做）

1. 确认 metrics 任务状态：
   ```bash
   python ./.trellis/scripts/task.py list --mine
   ```
2. 处置 `07-19-metrics-low-cardinality-audit-v1`（二选一，用户已可默认选 a）：
   - **a. 收尾**：按该任务 AC commit → `task.py archive` → ROADMAP 移入 Done
   - **b. 暂停**：ROADMAP Keep-Later 一行 + 可选 stash + `task.py finish`（若仍 current）
3. 确认无第二 in_progress 后，再 `task.py start 07-19-availability-first-convergence`。

> 规划阶段可先写好 diff 草稿，但 **提交 ROADMAP Now 切换** 必须在 gate 之后。

## 1. 编辑 `docs/ROADMAP.md`

按 `design.md` §3：

- [ ] 重写「优先级定义」表（P0=持续可用）
- [ ] 新增「Availability-First」节：目标句、健康定义、L0–L4、保/冻/后做
- [ ] 更新 Current Planning Decision 指向本收敛
- [ ] Now = 本任务（执行期）；注明收尾后置空规则
- [ ] Ready = P0-A / P0-B / P0-C（目标+非目标+验收草稿+顺序）
- [ ] 降级 `api-readonly-contract-minimal-v1`
- [ ] Keep-Later 表保留；加一句「可用性收敛期默认不 Resume」
- [ ] 不删 Done 历史；metrics 收尾后写入 Done

## 2. 任务元数据

- [ ] 本任务 `notes` / prd 与 ROADMAP 一致
- [ ] 可选：`task.py set-scope` 已是 `docs`
- [ ] 不创建代码子任务目录（除非用户当场要求 create P0-A）

## 3. Context manifests（start 前）

```bash
python ./.trellis/scripts/task.py add-context 07-19-availability-first-convergence implement docs/ROADMAP.md "Primary edit target"
python ./.trellis/scripts/task.py add-context 07-19-availability-first-convergence implement docs/proxy-usage.md "Usage vs free-pool honesty"
python ./.trellis/scripts/task.py add-context 07-19-availability-first-convergence implement docs/ops-cleanup.md "pool.tier semantics"
python ./.trellis/scripts/task.py add-context 07-19-availability-first-convergence implement .trellis/spec/proxy-core/backend/scenario-quality-tiers.md "QualityTier exit tables"
python ./.trellis/scripts/task.py add-context 07-19-availability-first-convergence implement .trellis/spec/proxy-gateway/backend/index.md "Gateway responsibilities"
python ./.trellis/scripts/task.py add-context 07-19-availability-first-convergence check docs/ROADMAP.md "Verify queue and principles"
python ./.trellis/scripts/task.py add-context 07-19-availability-first-convergence check .trellis/tasks/07-19-availability-first-convergence/prd.md "AC checklist"
python ./.trellis/scripts/task.py validate 07-19-availability-first-convergence
```

（规划阶段可先写入 jsonl；上表为规范命令。）

## 4. Validation

- [ ] 人工对照 AC1–AC8
- [ ] `rg -n "Availability-First|reliable-exit-defaults|gateway-failure-feedback|dirty-window" docs/ROADMAP.md`
- [ ] 确认 diff 无 `crates/**` 业务改动：
  ```bash
  git diff --name-only
  ```
- [ ] 1-WIP：`task.py list --mine` 仅一条 in_progress（本任务）

## 5. Finish

- [ ] Commit：`docs(roadmap): availability-first convergence`（或拆 chore）
- [ ] `task.py archive` 本任务
- [ ] ROADMAP Now 置空；提示下一条 `task.py create/start` → P0-A
- [ ] 不 apply stash

## Rollback

```bash
git checkout -- docs/ROADMAP.md
# 任务目录按需保留或回滚
```

## Out of band（明确不做）

- `cargo test` 非必须（无代码）
- 不启 dev 服务
- 不调用 `update_service`
