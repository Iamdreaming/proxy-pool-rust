# Implement: reliable-exit-defaults-v1

## 0. Gate

- [x] 任务已 create：`07-19-reliable-exit-defaults-v1`
- [x] 用户审阅 prd/design/implement 并批准 start（含 default+GeoIP 中国→Direct）
- [x] `task.py start` 后唯一 in_progress
- [x] ROADMAP Now 写入本任务（start 后第一步文档）

## 1. Context manifests

```bash
TASK=07-19-reliable-exit-defaults-v1
python ./.trellis/scripts/task.py add-context $TASK implement config/routes.example.yaml "Primary default routes profile"
python ./.trellis/scripts/task.py add-context $TASK implement config/settings.example.yaml "routes_path operator hint"
python ./.trellis/scripts/task.py add-context $TASK implement README.md "Routing decision section"
python ./.trellis/scripts/task.py add-context $TASK implement .trellis/spec/proxy-core/backend/scenario-quality-tiers.md "Tier exit tables"
python ./.trellis/scripts/task.py add-context $TASK implement docs/ROADMAP.md "P0-A queue item"
python ./.trellis/scripts/task.py add-context $TASK check config/routes.example.yaml "AC1-AC3"
python ./.trellis/scripts/task.py add-context $TASK check README.md "AC5 tier-aligned copy"
python ./.trellis/scripts/task.py add-context $TASK check .trellis/tasks/07-19-reliable-exit-defaults-v1/prd.md "AC checklist"
python ./.trellis/scripts/task.py validate $TASK
```

## 2. Implementation order

1. **ROADMAP** — Now = 本任务。
2. **`route_debug.rs`** — default 命中：helpers →（非 direct-only 则）GeoIP → 组策略；补单测 AC10/AC8。
3. **`routes.example.yaml`** — overseas-stable 主 profile + domestic-friendly 注释。
4. **`settings.example.yaml`** — `routes_path` + L1/GeoIP 提示。
5. **README** — 路由决策链 + GeoIP default 说明。
6. **测试** — example 文件加载 + GeoIP default。
7. **校验** — 见 §3。
8. **ROADMAP 收尾** — P0-A → Done；Now 空。
9. **commit + archive**（`feat(core): geoip-aware route default + overseas-stable example` 或拆 docs/test）。

## 3. Validation commands

```bash
cargo test -p proxy-core routes_example -- --nocapture
cargo test -p proxy-core --lib route_debug::
cargo test -p proxy-core --lib router::
cargo clippy -p proxy-core --all-targets -- -D warnings
cargo fmt --all -- --check

rg -n "default|premium|overseas|GeoIP|geoip" config/routes.example.yaml
rg -n "routes_path" config/settings.example.yaml
rg -n "路由决策|premium|GeoIP|Xray" README.md
```

## 4. Review gates before done

- [x] AC1–AC10 勾选
- [x] 非 default 规则行为无回归（现有 route_debug 测试全绿）
- [x] domestic-friendly default∈direct 不被 GeoIP 改写
- [x] example 头注释含 WARP/xray + GeoIP 说明
- [x] 1-WIP 保持
- [x] trellis-check 通过
- [ ] commit + archive + ROADMAP Done

## 5. Rollback

```bash
git checkout -- config/routes.example.yaml config/settings.example.yaml README.md docs/ROADMAP.md
# revert test commit if needed
```

## Out of band

- 不 push 除非用户要求
- 不 SSH / update_service
- 不 start P0-B
