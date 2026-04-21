---
name: guandan
slug: guandan
description: >-
  Play GuanDan(掼蛋) via clawguandan CLI as an AI player. Use when users ask to plan GuanDan or create/list/join tables in game.
metadata:
  openclaw:
    emoji: "🃏"
    requires:
      bins: []
    os:
      - linux
      - darwin
      - win32
---

# Guandan

你是通过 clawguandan CLI 玩 Guandan游戏 的 AI Player。

## 硬规则

1) 只用 CLI 交互， 命令是 clawguandan ，不臆测状态。
2) 仅以 CLI 返回的 JSON 为真相。
3) 默认快速决策；用户要求深思时再放慢。
4) 提及 **ID**（如 `tableId`、`playerId`）时，必须从原文 **逐字逐符** 复制；不要概括、不要猜测 ID 格式。

## 可用命令

```
clawguandan table create "<name>"
clawguandan table join -t <tableId> --name "<playerName>" --type ai --model "<llmModelName>"
clawguandan play ready -t <tableId> -p <playerId>
clawguandan play wait4myturn -t <tableId> -p <playerId>
clawguandan play playcards -t <tableId> -p <playerId> "<c1,c2,...>"
clawguandan play pass -t <tableId> -p <playerId>
clawguandan play tribute -t <tableId> -p <playerId> "<card>"
clawguandan play returncard -t <tableId> -p <playerId> "<card>"
clawguandan show rules [--lang en|zh]
```

- `show rules` 输出内置的精简规则（**Markdown**）到 stdout；不访问 server，**不是**对局 JSON。
- 你不应该使用以上命令以外的命令，也不需要自己探索命令参数，或者Web Search相关命令！

## 游戏启动流程

1) 删除TOOLS.md文件中的`### guandan pending ###`小节（如果有）并忽略它的内容
2) 先确认是你创建牌桌，还是直接告诉你牌桌号(tableId)
  - 如果你创建牌桌，执行：`clawguandan table create "<name>"` ，并得到tableId
  - 如果告诉了你牌桌，直接进行下一步
3) 跟用户明确你需要创建多少个AI玩家加入游戏，为每个玩家起一个简短又酷的名字(playerName)
4) 为每个玩家加入游戏，执行：`clawguandan table join -t <tableId> --name "<playerName>" --type ai --model "<llmModelName>"`
5) 为每个玩家启动(sessions_spawn)一个Subagent任务(run mode，Label用玩家playerName)，构建完整的task prompt, 内容须包括：
  - **显式**提示 `Read Guandan Skill， 并执行 "任务管理策略"`
  - `tableId`, `playerId` 等关键信息，务必精确，加双引号
  - **显式**提示 `不要轻易执行 "sessions_yield"`
6) 更新TOOLS.md，新写入`### guandan pending ###`小节，内容须包括：
  - **显式**提示 "Read Guandan Skill， 并遵循`任务管理策略`
  - `tableId`, 及每个玩家的：`playerId`, `playerName`, `Subagent`，务必精确，加双引号
7) 总是遵循下面的`任务管理策略`

### 任务管理策略

1) 每当收一个Subagent完成的消息，如果没有[GAME OVER]则再次启动(sessions_spawn)一个新的Subagent，构建完整的task prompt, 内容须包括：
  - **显式**提示 "Read Guandan Skill， 并执行 Subagent execution flow"
  - tableId, playerId等关键参数，务必精确，加双引号
  - 刚完成的Subagent返回的总结信息
  - **显式**提示 "不要轻易执行 sessions_yield"
2) 如果用户询问游戏状态，或者遇到游戏问题：
  - 主动查看所有Subagents的状态
  - 对已退出或已卡住的Subagent，执行上述一样的再次启动动作
3) 如果收到[GAME OVER]的通知，或者用户要求结束游戏：
  - 告诉用户游戏结果，做个简单总结
  - 删除TOOLS.md文件中的`### guandan status ###`小节（如果有）

## 游戏恢复流程

1) 尝试从上下文`### guandan pending ###`小节中读取关键游戏参数
2) 明确要恢复的`tableId`, `playerId`等，必要时请求用户提供
3) 遵循`任务管理策略`来进行Subagent相关确认和恢复

## Subagent执行流程

1) **重复**执行以下流程最多3次:
  1. 执行：`play wait4myturn -t <tableId> -p <playerId>`
  2. 读取返回状态 JSON，重点看：
    - status / phase
    - expect.kind
    - expect.actorPlayerIds
    - expect.legalActions 
    - private.handCards（若有）
    - hand.topPlay（若有）
  3. 若你不在行动集合中（playerId 不在 actorPlayerIds 中）则继续 wait4myturn
  4. 若 expect.kind 需要你行动，则按下述决策策略执行一个动作（动作必须在expect.legalActions中）

2) sessions_yield结束执行并返回结果
  - 若游戏未结束，须**显式**返回结果：`<playerId>: 游戏进行中，请为我再次启动(sessions_spawn)Subagent`
  - 若游戏结束，须**显式**返回结果：`[GAME OVER]`

### 决策策略

根据expect.kind来判断：
1) 若为 ready， 执行：`clawguandan play ready -t <tableId> -p <playerId>`
2) 若为 tribute，执行 `clawguandan play tribute -t <tableId> -p <playerId> "<card>"`
  - 出最大单张，尽量避开红心级牌/关键百搭。
3) 若为 exchange，执行 `clawguandan play returncard -t <tableId> -p <playerId> "<card>"`
  - 返最小低价值单牌，且不同点于收到的 tribute 牌。
4) 若为 play，执行 `clawguandan play playcards -t <tableId> -p <playerId> "<c1,c2,...>"` 或 `clawguandan play pass -t <tableId> -p <playerId>`
  - 若你是新一轮领出（topPlay 为空或由你方刚拿到牌权）：
    1. 优先出最小的“非炸弹”合法组合（单张 > 对子 > 三张/顺子等，尽量小出）。
    2. 保留炸弹和高牌（王、级牌、关键百搭）作为后手反制。
  - 若需要压过 topPlay：
    1. 先找能压过的同类最小组合；
    2. 没有同类可压，再考虑最小炸弹；
    3. 若炸弹代价过高且局面不关键，选择 `pass`。
  - 不确定合法性时优先 `pass`。

### 出牌风格

- 目标是“我方两人尽快出完”，不是只追求自己第一。
- 队友（对家）明显优势时，少抢牌权、少浪费炸弹。
- 对手要走牌时再用炸弹拦截。
- 不在非关键轮次用高价值资源（王炸/大炸/关键百搭）。

### 错误恢复

- 动作失败第 1-2 次：继续 `play wait4myturn -t <tableId> -p <playerId>` 后重算动作。
- 连续失败 >= 3 次：输出状态摘要 + 上次命令，停止盲试并请求用户决策。
