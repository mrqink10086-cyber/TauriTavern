# 状态机

状态文档回答"现在什么是真的"，状态机回答"故事走到哪一步"。两者正交，共用同一份[状态声明](State.md)与同一份存储：

> 这份 spec 通常**写在声明文档里**（`declaration.machine`），随场景一起保存与绑定：一场戏的字段、显示和规则是一件事。单独的命名机器与它自己的绑定仍然可用——声明没带机器时就回落过去，见 [State.md 的一份声明就是一个场景](State.md)。

| | 状态文档 | 状态机 |
| --- | --- | --- |
| 内容 | 字段的值（日期、地点、好感度） | 当前所处的位置（初见、试探、依恋） |
| 谁写 | 模型，每轮可变 | 只有转移能移动，代码确定性求值 |
| 存储 | `state/document.json` | `state/machine.json` |
| 校验 | 键是否在声明中 | 状态/条件/动作的定义是否成立 |

状态机是**可选层**。没有配置机器时，一切与只有状态文档时完全相同。

## 数据驱动

机器的一切行为都来自一份配置，代码里没有一个状态名、一条转移规则：

```json
{
  "initial": ["初见"],
  "states": [
    { "id": "初见", "label": "初见" },
    { "id": "试探", "label": "试探" },
    { "id": "依恋", "label": "依恋" }
  ],
  "transitions": [
    {
      "id": "warm-up",
      "from": ["初见"],
      "to": ["试探"],
      "conditions": [
        { "source": "field", "field": "关系/好感", "op": "gte", "value": "30" }
      ],
      "actions": [
        { "kind": "setField", "target": "关系/阶段", "values": ["试探"] },
        { "kind": "emit", "target": "进入试探" }
      ],
      "priority": 10
    },
    {
      "id": "trust-fall",
      "from": ["试探"],
      "to": ["依恋"],
      "conditions": [
        { "source": "active", "op": "active", "values": ["试探"] },
        { "source": "field", "field": "关系/信任", "op": "eq", "value": "稳固" }
      ]
    }
  ],
  "hooks": {
    "script": "export default (args) => ({ allow: true });"
  }
}
```

要点：

- **状态数量没有上限。** 域层没有任何字段数、状态数、转移数的业务上限（80 个状态的清单与 79 条链式转移都有测试覆盖）。`MAX_EVALUATION_ROUNDS` 只限制一次求值里"转移触发下一轮转移"的链长，循环配置会报 `state_machine.round_limit` 而不是被静默截断。
- **`from` / `to` 都是数组**，可空、可多。空 `from` 是无条件进入规则，多个 `to` 或同时活跃的多个位置就是并行区域——不需要额外的"并行"机制。
- **`initial` 可以有多个**，同样用于并行区域。
- **`terminal` 只是展示标记**，求值器不特殊对待它。"离开不了这个状态"是业务规则，由"没有出边"表达，不由代码假设。
- **同一轮多条转移争同一个位置时按 `priority` 取舍**，落选者记为 `position_taken` 而不是消失；平局按声明顺序，结果可复现。

## 条件词汇表

条件 = `source` + `op` + 操作数。`op` 走注册表解析（一张 map，不是 `match` 枚举），未知 `op` 在保存时报错并列出全部可用值。

| source | 读什么 |
| --- | --- |
| `field` | 状态文档里的字段，键由 `field` 指定，必须在声明中 |
| `active` | 机器自己的当前位置 |

内置 `op`（通用比较，不含任何业务含义）：

| op | 含义 |
| --- | --- |
| `eq` / `ne` | 字段首个值等于 / 不等于 `value` |
| `in` / `not_in` | 字段任一值在 / 不在 `values` 中 |
| `contains` / `not_contains` | 字段任一值包含 / 不包含 `value` 子串 |
| `gt` / `gte` / `lt` / `lte` | 数值比较（无法解析为数字即不满足） |
| `exists` / `missing` | 字段有非空值 / 没有 |
| `matches` | 字段任一值匹配 `value` 给出的正则 |
| `active` / `inactive` | （source 必须是 `active`）列出的位置都活跃 / 都不活跃 |

内置动作：

| kind | 含义 |
| --- | --- |
| `setField` | 把 `target` 字段写为 `values`（空数组即清空） |
| `clearField` | 清空 `target` 字段 |
| `emit` | 产生一个名为 `target` 的事件，随求值结果返回 |

`source` 与 `op` 不匹配（例如用 `active` 去读字段值）在保存时报错。

## 脚本钩子

`hooks.script` 是一段 JS，在 QuickJS 沙箱里执行（复用 Skill 脚本引擎）。它有两种被调用的理由，用 `args.reason` 区分：

| `reason` | 什么时候 | 它能做什么 |
| --- | --- | --- |
| `transition` | 每一条即将生效的转移 | 否决这一条；也可以顺手写几个字段 |
| `recalculate` | 界面写入之后（一个字段被改完时） | 只算：把派生值写回去 |

```js
export default function ({ reason, transition, to, active, fields }) {
  if (reason === 'recalculate') {
    // 人改了某个字段；把算得出来的那几个补上。
    const str = Number(fields['角色/属性/力量']?.[0] ?? 0);
    return { writes: [{ key: '角色/数值/攻击', values: [str * 2 + 5] }] };
  }

  const trust = fields['关系/信任']?.[0];
  if (trust === '破裂' && to.includes('依恋')) {
    return { allow: false, reason: '信任已破裂，不能进入依恋' };
  }
  return { allow: true };
}
```

契约很小，故意如此：

- `{ allow: false, reason }` 即否决；被否决的转移记为 `denied_by_hook`，其动作一个都不生效。
- `{ writes: [{ key, values }] }` 是**写入**：值可以是字符串、数字或布尔，空数组表示清空（与状态文档的三态语义一致）。
- 一次转移调用的写入**属于那条转移**：那条转移没生效（被否决、或在第二趟里被更高优先级的规则抢走），它的写入也不落地。
- `recalculate` 调用没有转移可否决，`allow` 在这一趟没有意义，只读 `writes`。
- 返回其他任何东西都视为**放行**——脚本抛异常是调用方必须看见的错误，不是一次含义不明的否决；`writes` 写法读不出来时同样报错（`state_machine.hook_invalid_writes`），不静默丢弃。
- 声明了钩子但没有可用引擎时报错 `state_machine.hook_unavailable`，不静默跳过。
- 钩子拿不到文件系统与宿主对象，只有 `args` 里的那点数据。

领域层只负责算出"哪些转移被选中"；跑脚本、收集否决与写入、再算一遍由应用层完成。所以钩子无法破坏求值的确定性——它只能说"这条不要"和"这几个值写上"。

**写入走的是同一条路**：钩子的 `writes` 并入求值结果的 `writes`，由调用方交给状态文档的 `resolve_request` + `apply_update`。钩子不能触碰声明没定义的键，也没有自己的存储。模型侧的 `state.transition` 还会再过一个逐字段可写授权；Run 提交时的自动求值不过（那是用户自己写的规则在行动），界面写入也不过（那是人点了一下）。三种路径都过声明校验——它管的是存储的形状。

## 接口

**Tauri 命令**

| 命令 | 用途 |
| --- | --- |
| `save_state_machine` | 注册/更新一份机器（带完整校验，不合法直接拒绝） |
| `get_state_machine` / `list_state_machines` / `delete_state_machine` | 命名存储的读写 |
| `validate_state_machine` | 只校验不落盘，返回全部问题 |
| `evaluate_state_machine` | 跑一次：解析显式请求 → 求值 → 返回结果 |

`evaluate_state_machine` 的参数：

```json
{
  "machine": { "...": "同上" },
  "active": ["初见"],
  "fields": { "关系/好感": ["42"], "关系/信任": ["稳固"] },
  "requests": [{ "to": ["依恋"] }]
}
```

- `active` 省略时用 `machine.initial`。
- `requests` 是显式发起的转移（模型或脚本）；省略则只跑规则自身触发的转移。
- 返回 `{ evaluation, errors }`：`evaluation.active` 是新位置，`applied` 是生效的转移，`skipped` 带原因（`position_taken` / `denied_by_hook`），`writes` 是需要写回状态文档的字段写入，`events` 是 `emit` 产生的事件；`errors` 是请求本身的错误。

**批量更新与转移校验**

一次调用可以提交任意多条 `requests`，全部问题一次列出（与 `state.update` 的错误回灌一致）：

| 情况 | 错误码 | 说明 |
| --- | --- | --- |
| 请求了不存在的状态 | `state_machine.undefined_state` | 指明是哪个 id、哪条请求 |
| 当前位置下没有这条转移 | `state_machine.illegal_transition` | 消息里带上当前活跃位置 |
| 引用了未声明的字段 | `state_machine.undeclared_field` | 条件读取与动作写入都查 |
| 未知 `op` / 动作 / source | `state_machine.condition_op_unknown` 等 | 消息里列出全部可用值 |
| 重复状态 id、空 id | `state_machine.state_id_duplicate` 等 | |
| 机器无法启动 | `state_machine.no_entry` | 既无 `initial` 也无无条件进入的转移 |
| 转移互相激活不停 | `state_machine.round_limit` | 明确报错，不截断 |

`writes` 不由状态机直接落盘：调用方必须把它们交给状态文档的写入路径，让同一份声明校验再过一遍。状态机不能绕过声明写字段。

## 扩展方式

三种粒度，越往下越不需要改代码：

1. **改配置**：加状态、加转移、改条件与动作。绝大多数需求到此为止。
2. **注册比较器**（代码扩展）：`StateMachineService::register_comparator("isLong", |ctx, condition| …)`。`op` 随即成为合法值，可在配置里使用。这是给"通用比较器表达不了，但仍属通用判定"的情况准备的。
3. **脚本钩子**：业务专属规则写在 JS 里，见上。钩子能否决，也能读全部字段做任意判断——代价是它不受声明校验保护，也不可确定性回放，所以只应放"通用机制真表达不了"的那一小部分。

## 边界

- **不枚举状态名。** 域层没有"初见/试探/依恋"这类概念，`id` 是配置里的字符串。
- **不替代状态文档。** 位置不等于值；想展示好感度就声明字段，不是建几十个状态。
- **不在求值里猜。** 条件不满足就是不满足，不做相似度匹配、不挑"最相近"的转移。
- **不静默降级。** 未知 `op`、未知动作、声明了钩子却没有引擎，都是错误。
- **动作不直接写存储。** 只产出 `writes`，由状态文档链路校验后再落盘。
- **不预设执行者。** 谁推进机器由调用方决定：命令、脚本，或模型发起的请求。

## 接线

以下四块把这一层接到运行路径上。命令与前端路由（`/api/state-machines/{save,get,list,delete,validate,evaluate}`）保持独立可用：机器可以在界面上保存、校验与求值，不必先绑定到聊天——这是"可选层"在开发期的形态。

- **按聊天绑定机器**：绑定沿用声明的"聊天 → 角色/群组 → 无 fallback"策略（`src/scripts/state-machine-binding-policy.js`，候选来自 `power-user.js` 的 `getStateMachineBindingCandidates()`），且仅当名字仍在 `list_state_machines` 里才算数。快照里放的是**完整 spec**（`stateMachine` 键），与 `stateDeclaration` 同构——后端因此不必自己去查存储，run 内的行为也不会被中途改配置影响。任何一步失败都 warn 并省略该键：一个看不见绑定的 run 不会去发明一个。
- **位置持久化**：`state/machine.json`（`MachineState`），与状态文档同根同机制——随 run 发布、由下一楼继承（`state/` 是持久根，run 启动时 base 版本的文件会被复制进运行工作区）。读不到位置文件时取 `initial_state(spec)`，所以第一个 run 不需要特例。
- **模型侧工具 `state.transition`**：请求形状与 `evaluate_state_machine` 的 `requests` 相同（`to` 必填、`from` 可选）。请求先经 `resolve_requests` 解析成转移下标，不合法就整批回灌且什么都不动；随后求值（钩子照常可否决）。转移携带的字段写入在**位置移动之前**过一遍状态文档的校验与当前 Profile 的可写授权——任何一步被拒都是"没动过"，模型可以一轮改对。未绑定机器时返回可恢复错误，而不是停机。
- **Run 提交时自动求值**：run 结束、发布之前跑一次，只跑规则自身触发的转移（`forced` 为空）——模型显式请求的移动已经在上面的工具里发生。写入经状态文档校验后落盘，位置与值一起随该楼发布。**失败不带走已提交的正文**：错误记入 run journal（`state_machine_advance_failed`）后继续提交——状态允许落后于剧情，但失败不允许静默。

**可写授权只约束模型发起的写入。** `state.update` 与 `state.transition` 都要过 `StateAccess`（"这个 Profile 的模型能写什么"）；run 结束的自动求值**不**过它——那是用户自己配置的规则在行动，不是模型的写入，拒绝它等于让用户写下的规则无法执行。声明校验三条路径都要过：它管的是存储的形状。
