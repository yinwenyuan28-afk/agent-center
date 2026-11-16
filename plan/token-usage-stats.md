# token 用量统计

2026-11-12 实施。目标是看清「哪个 agent、哪个模型、哪天花了多少 token」。

首版覆盖 **zcode、pi、codex、claude**，第二轮加了 **cursor（估算）**。其余 agent 在界面上
明确标「暂不支持」，不假装有数据。

## 设计决定

### 不自己记账，只读各家已有的数据

每个 agent 在调用模型后都会把用量写进自己的 session 记录，位置和格式各不相同。
再维护一份自己的账本等于抄两遍，还会因为崩溃丢记录。所以统计模块是纯读取器。

代价是格式随各家版本漂移，所以每条读取逻辑都单独写了容错：读不到、格式不对、
文件只写了一半，都不中断整页统计，只把问题记在那个 agent 的状态行上。

### 默认一家都不统计

这些数据源加起来上 GB（本机 pi 595 MB + codex 191 MB + zcode 1 GB 的库）。
读不读、读谁，由用户显式勾选决定，写进 `~/.agents/agentcenter.toml` 的 `[usage] enabled`。

「没启用就不读盘」是硬约束：`collect_report` 在没启用时连数据源都不 stat，
而不是读完再丢弃。

## 四家的数据位置（实测）

| agent | 位置 | 结构 | 本条用量在哪 |
|---|---|---|---|
| **ZCode** | `~/.zcode/cli/db/db.sqlite` 表 `model_usage` | SQLite，26557 行 | `input_tokens` / `output_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens` / `model_id` / `started_at`(ms) / `status` |
| **pi** | `~/.pi/agent/sessions/<项目>/**/*.jsonl` | 每行一个事件 | `type=="message"` 且 `message.role=="assistant"` → `message.usage.{input,output,cacheRead,cacheWrite}`，模型在 `message.model` |
| **Codex** | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | 每行一条 rollout | `type=="token_usage_record"` → `payload.usage.{input_tokens,cached_input_tokens,cache_write_input_tokens,output_tokens}`；模型来自 `turn_context.payload.model` |
| **Claude** | `~/.claude/projects/<项目>/<会话>.jsonl` | 每行一个事件 | `type=="assistant"` → `message.usage.{input_tokens,output_tokens,cache_read_input_tokens,cache_creation_input_tokens}`，模型在 `message.model` |

ZCode 走 SQLite，需要 `rusqlite`（bundled，静态编入 SQLite）。其余三家读 JSONL。

### Cursor：只能估算

Cursor 按**请求次数**计费，不按 token 记账，所以本地没有单次 token 用量。实测三处全空：

| 字段 | 数量 | 值 |
|---|---|---|
| `bubbleId:*` 的 `tokenCount` | 136,139 条 | 全部 `{inputTokens: 0, outputTokens: 0}` |
| `bubbleId:*` 的 `tokensUsed` | 753 条 | 全部 `null` |
| `agent-transcripts/*.jsonl` | 461 文件 37,988 行 | 完全没有 token 字段 |

唯一有真实值的是 `contextWindowStatusAtCreation.tokensUsed`（753 条非零）。
它是「这条消息发出时上下文窗口里有多少 token」，同一会话内递增：

```
03:40:38  tokensUsed=94595
03:41:23  tokensUsed=98691   +4096
03:43:00  tokensUsed=107156  +8465
07:23:07  tokensUsed=21494   -89639   ← 上下文压缩后回落
```

估算方法：**相邻快照递增量的和**。只算 `tu > prev` 的段，压缩造成的回退跳过。
实测 57 个会话有数据，递增 629 次、递减 52 次，估算总量 9,847,986。

这是**下界**：首条消息的绝对值不计（不知道是不是从 0 开始），压缩掉的上下文也不计。
`output` 本地完全没有记录，恒为 0。界面上写明是估算值。

**性能**：那个库 4.2 GB，`contextWindowStatusAtCreation` 没有可用索引，
全表扫描 22~30 秒（Python 测）/ 3.5 秒（Rust release）。所以 Cursor 走增量扫描。

## 增量缓存

第二版改成增量。第一版的做法是「文件指纹变了就整份重读」，
实测每次打开统计页的耗时是：

| agent | 全量 | 增量 |
|---|---|---|
| zcode | 0.13s | 0.12s（本来就不做增量，见下） |
| pi | 1.72s | **0.17s** |
| codex | 0.06s | 0.05s |
| claude | 0.08s | 0.06s |
| cursor | 3.55s | **0.06s** |
| 五家一起 | 37.00s | **0.26s** |

「五家一起」的全量比逐家相加慢得多（37s vs 5.5s），因为 Cursor 那 4.2 GB
的库在同时读其他库时磁盘竞争严重。

缓存放在 `~/.agents/usage-cache/<agent>.json`。实现在 `src-tauri/src/usage/cache.rs`。

### JSONL 文件：记字节偏移，只读尾巴

pi / codex / claude 的会话文件是**纯追加写**的，旧字节永远不变。
所以缓存里给每个文件记三样东西：

- `offset`：已消费到第几个字节，永远停在某个换行之后
- `mtime_ns`：上次读完时的修改时间
- `head_hash`：已消费部分前 4 KB 的 FNV-1a 哈希

下次运行：

1. `offset == size && mtime == 现在的 mtime` → 文件没动，直接用缓存，**零次磁盘读**
2. 文件变长了且 `head_hash` 对得上 → 从 `offset` 往后读，只解析新增的行
3. 文件变短或头部哈希对不上 → 说明被截断/整体改写，从头解析

**为什么只哈希前 4 KB**：完整比对旧内容的开销等于重读一遍，增量就白做了。
而会话文件被整体改写时，开头几乎必然跟着变。

**不用 `DefaultHasher`**：它的输出在不同 Rust 版本间不保证稳定。
拿它算的哈希存到磁盘，换个编译器版本就全部对不上、缓存永久失效。
所以手写 FNV-1a。

**半行怎么处理**：进程正在写时，文件尾部可能只有半条 JSON。
只有读到换行才推进 `offset`；最后那段没有换行的内容，只有能完整解析成
JSON 才当作已读，否则留给下次。不这么做的话，一次半行写入会把后面
所有内容都读成错位的垃圾。

### Codex 的跨行状态必须一起存

`token_usage_record` 里**没有模型名**，模型在同文件前面的 `turn_context`
行里。只读文件尾巴时那些行早就读过了，所以「turn_id → 模型」的映射
必须跟着文件的 `offset` 一起持久化（`CodexState`）。

漏掉这一步不会报错，只会让新增的记录全部因为找不到模型被静默丢掉 ——
这是最危险的一种算错，因为界面上看不出任何异常。

### Cursor 的库：记 rowid 水位

`cursorDiskKV` 的 rowid 随插入单调递增（实测尾部 4000 行里 934/936 的
rowid 与 `createdAt` 同序）。所以记住上次读到的最大 rowid，
下次 `WHERE rowid > 水位` 只碰新行，实测 0.000 秒。

缓存里存的是**快照**（每个 composer 的 `tokensUsed` 序列），不是算好的差值。
这样新快照乱序到达也不会算错：每次都把所有快照按时间重排、重算差值。
快照总数不到一千条，重算可以忽略。

`ON CONFLICT REPLACE` 会让更新过的行换个新 rowid 被再读一次。
这不会重复计数：同样的 `(时间, tokensUsed)` 再出现一次，差值就是 0。
但**删除**的行不会被发现。真要纠正就跑 `--refresh`。

### ZCode 为什么不做增量

`model_usage` 表全量查询只要 0.05 秒（`started_at` 上有索引），
而它的行是「先插入 `running`、跑完再改成 `completed`」——
按 rowid 设水位会漏掉那些后完成的记录。

用 0.05 秒换一个不会算错的实现，比省下 0.05 秒值得。

### 什么时候会让缓存失效

- 文件变短 / 头部哈希不匹配 → 这个文件从头读
- 缓存版本号变了（解析逻辑升级）→ 整份作废
- 用户跑 `usage report --refresh` 或点界面上的「全量重算」→ 全部从头算

缓存写盘用「先进 `.json.tmp` 再改名」，进程写到一半被杀也不会留下半份 JSON。
写盘时机是 `collect_report` 结束时统一做，不是逐文件写 ——
pi 有 601 个会话文件，逐个写盘会把增量省下的时间又花回去。

## 口径归一：这一步不做就会算错

四家的 `input` 含义不一致：

| agent | `input` 含缓存？ | 验证依据 |
|---|---|---|
| pi | 否 | 36163 条样本全部满足 `totalTokens = input + output + cacheRead + cacheWrite` |
| Claude | 否 | `cache_read_input_tokens` / `cache_creation_input_tokens` 是并排字段 |
| ZCode | 是 | `computed_total_tokens` 恒等于 `input_tokens + output_tokens`；`max(cache_read - input) = 0` |
| Codex | 是 | `total_tokens` 恒等于 `input_tokens + output_tokens`；`cached <= input` |

统一折算成「**输入含缓存**」：

```
input_includes_cache = true  →  归一 input = input
input_includes_cache = false →  归一 input = input + cacheRead + cacheWrite
```

`cache_write` 也算进输入：它确实是被送进模型前的 token，只是计费打折。漏掉会让 pi 偏小。

## 三个必须知道的实现细节

### 1. Codex 的 usage 是单次请求量，不是累计值

`payload.usage` 每次调用一条。验证方法：拿一个 97 条记录的会话，
比较 `sum(每条 usage)` 与最后一条的 `thread_token_usage` ——

```
SUM  usage: in=8311231 out=31731 total=8342962
LAST thread_token_usage: in=8311231 out=31731 total=8342962   ← 完全相等
```

所以逐条累加正确。**不能再叠加 `turn_token_usage` 或 `thread_token_usage`**，那会翻倍。

### 2. 全 0 token 的记录要丢掉

pi 里占 3.4%（36163 条中 1241 条）：请求失败、被中断、指向本地模型的空回包。
ZCode 里也有 3 条 `status='completed'` 但两个 token 都是 0 的。

留着会让「调用次数」虚高，还会在界面上塞满 `0 / 0` 的行。
判断标准是四个原始字段（input/output/cacheRead/cacheWrite）全为 0。

只有 output 没有 input 的记录（如 Claude 的本地提示）是真实的，不能一起丢。

### 3. 时间按本机时区切日

用 UTC 的话，东八区早上 8 点前的用量会被算进前一天。窗口下界也对齐本地午夜：
`days=7` 表示含今天在内的 7 个自然日。

## 图表

第二轮加的。两个图都手写 SVG，不引图表库——构建产物只增加约 9 KB
（引入 ECharts 会多 350 KB gzip）。

### 折线图：每日 Token 趋势

- 横轴按天，纵轴是「输入 + 输出」
- **按模型分组**，每个模型一条线、一个颜色
- 缺数据的日期补 0，不断线——断线会让人误以为数据源缺失，其实是那天没用这个模型
- 平滑曲线用 Catmull-Rom 样条转贝塞尔，但**纵向控制点被夹在两端点取值区间内**。
  不夹的话样条会在陡升前先往下压，画出实际不存在的低谷
- 悬停显示竖线 + 各模型当天的数值

**量级差是真实信息**：不同模型可能差几个数量级（几十亿 vs 几千），
同轴叠画时小的那条会贴在 x 轴上。这是有意的——用户要看「谁吃得多」，
小模型的占比在饼图里体现。

### 饼图：模型用量占比

- 环形图，用 `stroke-dasharray` 画弧段（比手算扇形路径简单，不用处理大弧标志位）
- 中心显示总量，右侧配「模型 / 百分比 / 绝对量 / 调用次数」列表
- 最多 8 个模型，其余合并成「其他」，否则饼图会碎成一堆看不出比例的细条

两个图共用一套调色板，同一模型在两个图里颜色一致。

## 界面结构

第三次调整后的顺序（按用户反馈）：

1. 时间窗按钮（7 / 30 / 90 天 / 全部）
2. 三张汇总卡片（输入 / 输出 / 调用次数）
3. 折线图
4. 饼图
5. 按 agent 小计表
6. 按天 × 模型明细表
7. **「纳入统计的 agent」面板，默认折叠**

第 7 项放在最底部并收起，是因为它是配置动作，日常看的是上面的数字。
折叠头显示「N 家已启用」，不展开也能知道当前状态。

## 一条没有采用的优化

**不做「mtime 早于窗口就跳过整份文件」**，虽然那能省很多时间。
原因是这个假设靠不住：用户从备份恢复会话时（`cp -p` 保留原 mtime），
文件里记录的时间会晚于 mtime，那份数据会被静默漏掉。

增量是按「文件有没有变」判断的，不依赖记录时间，所以不受这个问题影响 ——
这也是它比「按时间跳文件」更可靠的地方。

## 验证记录

本机真实数据下逐家核对，均与独立脚本复算一致。

| 验证项 | 结果 |
|---|---|
| ZCode sqlite 直查 vs 统计输出 | 一致（2026-11-12 当天 1840 次调用、5.3 亿 input） |
| pi 2026-11-12 复算 | 完全一致：input 27,390 / output 70 / 1 次调用 |
| pi 总记录数 | 36,163 条，与独立扫描相同 |
| Codex 累加语义 | `sum(usage) == thread_token_usage`，逐条累加正确 |
| **Cursor 估算复算** | 完全一致：9,847,986（629 次递增） |
| 时间窗 | `--days 7` 从 2026-11-06 起，含今天，正确 |
| 开关写入 | `[usage] enabled` 格式与顺序稳定 |
| 关闭后不读盘 | pi 关闭后 CLI 耗时 0.16s（开启时 2.0s，release） |
| 拒绝不支持的 agent | `usage enable opencode` 退出码 1，提示「暂不支持」 |
| 空状态 | 全部关闭时显示引导文案而非空表格 |
| 契约检查 | 37 个类型 207 个字段一致 |
| 前端 | svelte-check 0 错误 0 警告；`npm run build` 通过（+9 KB） |
| **折线图** | 2 条曲线（蓝 #4d9dff / 绿 #3ecf8e）、5 个 y 刻度、4 个日期标签 |
| **悬停交互** | 竖线 + 2 个圆点 + 浮层显示当天各模型数值 |
| **饼图** | 底环 + 2 个彩色弧段，中心 13.90 亿 tokens，明细 93.4% / 6.6% |
| **日期标签边界** | 首尾改 start/end 对齐后全部落在 0..900 内（原最后一个到 903 被裁） |
| **折叠面板** | 默认 `aria-expanded=false`、0 个可见开关；展开后 4 个开关 |
| **增量 == 全量（五家一起）** | 逐行完全一致：212 行、合计 input 13,811,746,045 / output 52,388,830 / 62,257 次调用 |
| **增量加速比（五家一起）** | 37.00s → **0.26s**（142 倍） |
| **增量加速比（pi 单独）** | 1.72s → 0.17s（10 倍，601 个会话文件、36,163 条记录） |
| **增量加速比（cursor 单独）** | 3.55s → 0.06s（59 倍） |
| **追加后仍与全量一致** | 往真实 pi 会话追加 1 行（input 4242 / output 424），增量与全量都只看到这 1 条、数值相同；随后把文件 truncate 回原长度，探针行消失 |
| **半行不吞** | 写 `{"n":1}\n{"n":2` → 只解析出 1 条；补全文末换行后变成 2 条 |
| **截断退回全量** | 文件从 3 行改写成 1 行后，结果从 3 条变 1 条（不是拿旧偏移读出错位数据） |
| 缓存文件 | `pi.json` 5.2 MB / 601 文件 / 36,163 记录；`cursor.json` 49 KB / 57 composer / 水位 384371 |

界面在浏览器预览（假后端）里逐项点过：侧栏出现「Token 用量」、开关切换后汇总从
13.87 亿升到 15.74 亿、时间窗切换日期组数变化、全部关闭后显示空状态。
真实后端用 `agentcenter usage` 命令验证了五家的数据与耗时。

## 已知限制

1. **Codex 的 `.jsonl.zst` 压缩历史未纳入**。本机 135 个文件属于这种格式。
   要支持需引入 zstd 解压库（`ruzstd` 已验证可解压，见实施记录）。界面上写明了这一点。
2. **不算金额**。只有 pi 自带 `cost`，母版 `providers.toml` 里只有一条模型配了单价。
   单价不全时算总价会误导，所以首版只出 token。
3. **Cursor 是估算，且不含输出**。理由见上。
4. **`usage-cache/` 里的文件不进 git**，但也没进 `.gitignore` 的显式条目——
   它在 `~/.agents` 仓库里而非本项目，由用户的 `~/.agents/.gitignore` 决定。
5. **pi 首次统计约 1.7 秒**（release，595 MB / 601 个文件），之后增量约 0.17 秒。
   首次指的是缓存不存在或跑了 `--refresh`；换机器、换用户目录也会是首次。
6. **ZCode 只统计 `status='completed'`**。`cancelled` 和 `error` 的记录实测 token 全为 0，
   但保留这个条件是为了防止上游以后给失败记录也写用量。

## 实施记录

- 新增 `src-tauri/src/usage/mod.rs`（口径归一、聚合、缓存、启用名单）
- `types.rs` 加 `UsageRow` / `UsageAgentStatus` / `UsageReport`
- `Agent` trait 加 `supports_usage` / `usage_sources` / `usage_collect` / `usage_note`，
  四家各自实现
- `config.rs` 加 `[usage] enabled`
- IPC：`usage_report`（异步，走 `spawn_blocking`）、`set_usage_enabled`
- CLI：`usage status|report|enable|disable`
- 前端：`Usage.svelte`、`types.ts`、`ipc.ts`、`mock.ts`、侧栏入口
- 依赖：`rusqlite` 0.32（bundled）
- 单测 5 个：两种口径归一、同桶累加、窗口下界、不限窗口

### 第二版：改成增量（2026-11-13）

- 新增 `src-tauri/src/usage/cache.rs`：JSONL 字节偏移增量、文件指纹、FNV-1a、半行处理
- `usage/mod.rs` 删掉整份重扫的 `FILE_CACHE` / `cached_parse` / `cached_db_scan`，
  改为 `cache::cached_jsonl` / `cached_jsonl_stateful`，并加 `flush_jsonl_cache`
- pi / claude 改成「按行回调」，codex 增加 `CodexState`（turn_id → 模型）持久化
- cursor 改成 rowid 水位增量，缓存存快照而非差值
- `collect_report(days, refresh)`：`refresh` 清缓存全量重算
- CLI `usage report --refresh`；IPC `usage_report(days, refresh)`；
  界面加「全量重算」按钮（带确认）
- 单测新增 3 个：追加只读增量、截断退回全量、半行不吞

2026-11-12 首版

2026-11-13 增量版
