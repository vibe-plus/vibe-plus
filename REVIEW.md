# 观测面板评审备忘

> 2026-05-19 · 对照未提交 working tree 核实。  
> **结论**：基建约 **55%**（页面 + `vibe-observability` + 独立 DB 已落地）；**产品诉求 0/5**；**验收 0/6**（chip 深链 1 项部分）。

---

## 1. 员工任务单（按顺序做）

| 序    | 任务                                                                                                                                                             | 阻塞      | 触达文件（主）                                               |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- | ------------------------------------------------------------ |
| **A** | **Outcome 对齐**：`client.ts` 改用 `packages/protocol` 生成类型；`attempt-row` 改 `obs.outcome.*` 并补全 kebab-case 文案；`switch` 覆盖全部 outcome              | P0 裸 key | `api/client.ts`, `attempt-row.vue`, `Observability.vue` i18n |
| **B** | **实体可读**：`onMounted` 拉 `api.providers.list()`；`resolveProviderLabel` + `credentialPrimaryAccountLabel` 注入 `EntityChip`；去掉 `credential_id.slice(0,6)` | P0 可读性 | `Observability.vue`, `attempt-row.vue`                       |
| **C** | **M/R/C**：抽 `protocolWireLetter(wire)`（对齐 `provider-import-modal` 的 M/R/C）；attempt 行 + network 表使用                                                   | 产品 §2   | 新建小 util 或 `protocol-label.ts`                           |
| **D** | **下游补凭证**；attempt 行补 `requested_model` / `route_prefix`（可选同 PR）                                                                                     | P1        | `Observability.vue`, `attempt-row.vue`                       |
| **E** | **产品拍板调度**：是否改为「首波 1 路 → 后续并发 race → 终局波」；**确认后再动** `selector::build_waves`                                                         | 产品 §4   | `forward/selector.rs`, `mod.rs`                              |
| **F** | **波次 UI**（依赖 E）：`wave_size>1` 才显示波头，否则「尝试 #n」；同 wave 多行并列                                                                               | 产品 §4   | `Observability.vue`, `attempt-row.vue`                       |
| **G** | **前端目录**：迁至 `dashboard/features/observability/`；`registerEntityResolver` 用起来或删死代码                                                                | P2        | 目录迁移                                                     |

**建议**：A+B+C 可合并为 **一个前端 PR**（不动网关）。E 未拍板前不要改波次文案骗人。

---

## 2. 状态总表（唯一进度表）

| ID       | 事项                                      | 状态 | 备注                                       |
| -------- | ----------------------------------------- | ---- | ------------------------------------------ |
| **基建** | 观测页 + 6 Tab + KPI/波形                 | ✅   | `Observability.vue`, `tabs-*`, `sparkline` |
|          | 路由 / 侧栏 / 深链                        | ✅   | `/ui/observability`, `entity-links`        |
|          | `vibe-observability` + `observability.db` | ✅   | 插件写入；`records.rs` 已迁出 core         |
|          | `vibe-plugin-api` + `GatewayEvent`        | ✅   | `forward/mod.rs` emit                      |
|          | 网络表有 `wire` 列                        | ⚠️   | 有列，仍是 slug                            |
| **§1**   | 供应商/凭证可读                           | ❌   | 仅 realtime；UUID / 截断 id                |
| **§2**   | 协议 M/R/C                                | ❌   | attempt 行无协议字母                       |
| **§3**   | 凭证邮箱/label + 跳转                     | ❌   | `slice(0,6)`                               |
| **§4**   | 调度：首单后并发、终局收束                | ❌   | `WAVE_SIZE_CLOSED=1` 串行                  |
| **§5**   | 目录隔离                                  | ⚠️   | 后端 ✅；前端仍在 `dashboard/pages`        |
| **P0**   | outcome / i18n / 类型三重漂移             | ❌   | 见任务 A                                   |
| **P1**   | 下游凭证、Tab 重复、行信息不全            | ❌   | 见任务 D                                   |
| **P2**   | `UpstreamAttemptPhase` 前端过时           | ❌   | 随任务 A 一并改类型                        |

---

## 3. 验收清单

| #   | 通过标准                                               | 现况                |
| --- | ------------------------------------------------------ | ------------------- |
| 1   | 可重试失败后成功：能看出**并发波**，非 4×「第 n/1 波」 | ❌                  |
| 2   | 无 active realtime 时供应商名、凭证仍可读              | ❌                  |
| 3   | 协议为 **M/R/C**，非 slug                              | ❌                  |
| 4   | Outcome 友好文案，无 `observability.outcome.*` 裸 key  | ❌                  |
| 5   | `retryable-error` 等与 badge 语义/颜色一致             | ❌                  |
| 6   | chip 跳转 provider / credential / 观测深链             | ⚠️ 链有，标签不可读 |

---

## 4. 产品诉求（期望 vs 现状）

| #   | 你要的                                                             | 现状                                                        |
| --- | ------------------------------------------------------------------ | ----------------------------------------------------------- |
| 1   | 供应商、凭证用实体解析组件展示人类可读名                           | `EntityChip` 有，label 多为 UUID；未 `providers.list()`     |
| 2   | 协议用 **M / R / C**（Messages / Responses / Chat）                | 网络表有内部 `wire` slug；attempt 行无协议                  |
| 3   | 凭证可见：邮箱 / label，可点进供应商页                             | 仅 `credential_id` 前 6 位                                  |
| 4   | **第 1 次单路 → 第 2 次起并发 → 再一波必给终局**；不要像竞品逐个试 | 见下「调度」；UI 呈 `4 尝试 / 4 波 / 第 n/1 波` 串行        |
| 5   | 观测前后端分目录；core 与观测插件化、**双库**                      | 后端 `vibe-observability` + `observability.db` ✅；前端未拆 |

---

## 5. 附录（只解释一次）

### 5.1 调度：产品期望 vs 代码

**产品期望**

```
Wave 0: 1 pick
Wave 1+: fanout 并发 race
最后一波: 强制终局（成功或聚合失败）
```

**当前**（`selector::build_waves`）：按熔断分桶 — Open **4 并发** → HalfOpen **2** → Closed **1 串行**；**波与波之间串行**。  
用户场景（4 个 Closed pick）：→ 4 个 `wave_size=1` 的波 → UI 显示「第 2/1 波」、总延迟累加。**改 UI 前须先拍板是否改 forward（任务 E）。**

### 5.2 现象 → 根因（速查）

| 现象                                    | 根因                                                            |
| --------------------------------------- | --------------------------------------------------------------- |
| 供应商 UUID                             | `providerNameFor` 无映射时 `return id`                          |
| `observability.outcome.retryable-error` | 子组件键名 `observability.*` ≠ 父页 `obs.*`；i18n 缺 kebab-case |
| `第 2/1 波`                             | Closed 每 pick 单独成波 + UI `wave_index+1 / wave_size`         |
| `015af1eb`                              | `credential_id.slice(0,6)` 当 label                             |

### 5.3 已交付基建（清单）

`Observability.vue` · `attempt-row` / `sparkline` · `tabs-*` · `/ui/observability` · `useRealtimeStream` history · `crates/vibe-observability` · `crates/vibe-plugin-api` · `/_vp/observability/*` + legacy records · `observability.db` + migrate_from_legacy

### 5.4 目录索引

| 层                 | 路径                                                    |
| ------------------ | ------------------------------------------------------- |
| 前端页             | `apps/web/src/dashboard/pages/Observability.vue`        |
| 前端组件           | `components/observability/*`, `components/ui/tabs*`     |
| 后端               | `crates/vibe-observability/`, `crates/vibe-plugin-api/` |
| 调度（未按产品改） | `crates/vibe-core/src/forward/{mod,race,selector}.rs`   |
| 主库 / 观测库      | `vibe.db`（providers 等）· `~/.vibe/observability.db`   |

---

_未提交改动；commit 前按 §1 任务单与 §3 验收自测。_
