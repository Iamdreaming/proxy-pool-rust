# Design: 仓库过程债专项收敛

## Scope Boundary

| 层 | 是否改动 | 说明 |
|----|----------|------|
| 业务 Rust/Vue 逻辑 | 否 | 不 apply stash、不改行为 |
| `docs/ROADMAP.md` | 是 | 状态叙事 + 原则 + Keep-Later 表 |
| 任务目录 | 是 | `inventory.md` 台账 |
| `.gitignore` + 取消跟踪噪音文件 | 是 | `proxy_ip_suggest.txt` |
| 本地 worktree / temp / target | 是 | 不进 git（除 gitignore/rm --cached） |
| `.trellis/spec/**` | 否（默认） | 无新可执行业务契约；过程协议落在 ROADMAP |

## Artifacts & Single Sources

| 信息 | 权威位置 | 同步规则 |
|------|----------|----------|
| 当前 Now | `docs/ROADMAP.md` §Now | 与 Trellis current / 任务状态一致 |
| Keep-Later 明细 | 任务内 `inventory.md` | ROADMAP 表为摘要；冲突以 inventory 盘点日为准 |
| stash 身份 | `git stash list` 的 **message** | 禁止只写易变的 `stash@{n}` |
| 过程仪式 | ROADMAP §管理原则 | 不另建平行规范，除非用户要求 |

## ROADMAP Target Shape（D4）

保留章节骨架，收敛“当前信号”：

1. **管理原则** — 增补 1-WIP、暂停/完成仪式（R4）
2. **Current Planning Decision** — 缩为短段：本任务目标 + D1 不 drop stash + 不恢复功能
3. **Now** — 执行期：`process-debt-convergence`；收尾：空 + 引导从 Ready start
4. **Keep-Later（原 Paused Closeout）** — 表：

   `| slug | P | stash message | intent (1 line) | disposition | restore |`

5. **Ready / Next / Later / Parking / Done** — 保留；Later 修幽灵指针；Done 顶注“历史”
6. **Trellis 任务创建建议** — 压缩为“已过时，以 Now/Ready/Keep-Later 为准”或极短指针，避免 20+ 条假排序

不把 Done 大搬家到外部文件（D4）。

## inventory.md Schema

```markdown
# Process debt inventory (as of YYYY-MM-DD)

## Stashes (Keep-Later)
| stash message | approx files | disposition | linked roadmap slug | restore notes |
...

## Roadmap drafts without active task dir
| slug | location (Later/archive) | disposition | notes |
...

## Local cleanup log
| action | result |
...
```

## Local Cleanup Sequence（失败可停）

```
1. git worktree list  → 确认 2 个 agent worktree
2. 各 worktree: git status（无独有 WIP 才 remove）
3. git worktree remove <path>  （必要时 --force 仅当确认可弃）
4. rm -rf temp .tmp_verify protoc.exe（忽略已不存在）
5. cargo clean
6. git rm --cached proxy_ip_suggest.txt && .gitignore 追加
7. git stash list 快照写入 inventory（证明未 drop）
```

回滚：

- worktree：无法自动恢复旧 agent worktree；可接受（已停在旧 commit）。
- `cargo clean`：重建即可。
- `proxy_ip_suggest.txt`：blob 仍在 git 历史；可 checkout 恢复跟踪。
- ROADMAP/inventory：git revert 文档提交。

## Risk Register

| 风险 | 缓解 |
|------|------|
| 误 `stash drop` | 清单明确禁止；验收检查 `git stash list` 仍含 7 条 message |
| 误 apply 污染 tree | 禁止 apply；AC6 检查无业务代码 diff |
| worktree 有未提交修改 | remove 前 status；有货则记录并跳过，写入 inventory |
| ROADMAP diff 过大难审 | 按章节改；Done 不重写正文 |
| stash@{n} 漂移 | 全文用 message 键 |

## Compatibility

- 不改 API/MCP/配置契约。
- 不触发 dev 更新或 SSH。
- 可选最终 commit：`chore(docs): converge process debt roadmap and inventory`。

## Non-Goals (design)

- 导出 stash 为 patch 文件（策略 B，未选）
- 自动 Resume 任一 Keep-Later 项
- 清理整个 `.claude` 目录
