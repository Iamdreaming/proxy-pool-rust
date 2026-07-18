# Implement: 仓库过程债专项收敛

## Preconditions

- [ ] 用户已审阅 `prd.md` / `design.md` / `implement.md`
- [ ] `python ./.trellis/scripts/task.py start` 使状态为 `in_progress`
- [ ] Active task: `.trellis/tasks/07-18-process-debt-convergence`

## Checklist

### 1. 冻结盘点快照

1. `git stash list` → 写入 `inventory.md` 头部（日期 + 完整 list）。
2. `git worktree list` → 记录两 worktree 路径与 HEAD。
3. 确认 7 条 message 仍在；若有增减，更新 inventory 并暂停问用户（D1 假设 7 条）。

### 2. 本地清理（R3 / D3）

1. 对每个过期 worktree：`git -C <path> status --short`；干净则 `git worktree remove <path>`。
2. 删除 `temp/`、`.tmp_verify/`、根目录 `protoc.exe`（存在才删）。
3. `cargo clean`；记录前后 `du -sh target`（或等价）。
4. `git rm --cached proxy_ip_suggest.txt`；`.gitignore` 增加 `proxy_ip_suggest.txt`。
5. **禁止**：`git stash drop|clear|apply|pop`。

### 3. 写 inventory.md（R2）

1. 按 design schema 填 7 stash 行：disposition=`Keep-Later`。
2. 填 Later/archived 幽灵项行。
3. 填 cleanup log（成功/跳过/原因）。
4. restore notes：用 message 定位；提醒可能与 main 漂移，apply 前需新任务。

### 4. 改 ROADMAP.md（R1/R4/D2/D4/D5）

1. **管理原则**：加入 1-WIP、Parking、暂停仪式、完成仪式。
2. **Current Planning Decision**：缩写为本任务 + Keep-Later 不 drop。
3. **Now**：`process-debt-convergence`（过程治理）。
4. **Paused Closeout** → **Keep-Later** 表（与 inventory 对齐，可更短）。
5. **Later**：修正“任务目录存在”等错误；指向 archive 或 Keep-Later。
6. **Ready/Next**：不重排（D5）；仅修明显错误措辞。
7. **Done**：顶部一句“以下为历史完成”。
8. **任务创建建议**：压缩为过时声明或极短列表。

### 5. 自检（对应 AC）

```bash
git stash list
# 应仍见 7 条 wip: paused ...

git worktree list
# 应不再含 agent-aa1c... / agent-abbe...

git status
git diff --stat
# 不应出现 crates/** 或 web/src/** 的功能恢复
```

人工读 ROADMAP Now + 管理原则（AC1/AC7）。

### 6. 收尾（本任务完成时，非 start 时）

1. ROADMAP Now 置空 + 引导从 Ready start（D2 后半）。
2. 按项目惯例归档本任务。
3. 可选 commit：仅 docs/chore。

## Validation Commands

| 检查 | 命令/方式 |
|------|-----------|
| stash 未丢 | `git stash list` 含 7 message |
| worktree | `git worktree list` 无那两个 agent 路径 |
| 无功能代码 | `git diff --name-only` 白名单：`docs/ROADMAP.md` `.gitignore` `proxy_ip_suggest.txt` `.trellis/tasks/07-18-process-debt-convergence/**` |
| target | `du -sh target` 显著变小 |
| 测试门禁 | **不**要求本任务 `cargo test` 全绿（clean 后可能未重编） |

## Risky Files / Rollback Points

| 步骤 | 风险点 | 回滚 |
|------|--------|------|
| worktree remove | 误删有货 worktree | remove 前 status；有货跳过 |
| cargo clean | 下次编译慢 | 预期代价 |
| git rm --cached | 误删其它文件 | 仅限 `proxy_ip_suggest.txt` |
| ROADMAP 大编辑 | 误删 Done 内容 | 小步编辑；`git checkout -- docs/ROADMAP.md` |

## Do Not

- 不要 `stash apply` 验证草稿
- 不要开始 Ready 上的功能任务
- 不要 SSH / `update_service`
- 不要改业务 `.trellis/spec` 契约（默认）
