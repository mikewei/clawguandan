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

## 可用命令

```
clawguandan table create "<name>"
clawguandan table join -t <tableId> --name <playerName> --type ai --model <llmModelName>
clawguandan play ready -t <tableId> -p <playerId>
clawguandan play wait4myturn -t <tableId> -p <playerId>
clawguandan play playcards -t <tableId> -p <playerId> "<c1,c2,...>"
clawguandan play pass -t <tableId> -p <playerId>
clawguandan play tribute -t <tableId> -p <playerId> "<card>"
clawguandan play returncard -t <tableId> -p <playerId> "<card>"
```

- 以上命令都需要前台执行，不要后台执行
- 你不应该使用以上命令以外的命令，也不需要自己探索l令参数，或者Web Search相关命令！

## 游戏启动流程

1) 删除TOOLS.md文件中的`### guandan status ###`小节（如果有）并忽略它的内容
2) 先确认是你创建牌桌，还是直接告诉你牌桌号(tableId)
  - 如果你创建牌桌，执行：`clawguandan table create "<name>"` ，并得到tableId
  - 如果告诉了你牌桌，直接进行下一步
3) 再确认你需要创建多少个玩家加入游戏，为每个玩家起一个简短有趣的名字(playerName)
4) 为每个玩家加入游戏，执行：`clawguandan table join -t <tableId> --name <playerName> --type ai --model <llmModelName>`
5) 为每个玩家启动(sessions_spawn)一个Subagent任务(run mode)，构建完整的task prompt, 内容须包括：
  - **显式**提示 "Read Guandan Skill， 并执行 Subagent execution flow"
  - tableId, playerId等关键参数
  - **显示**提示 "不要轻易执行 sessions_yield"
6) 更新TOOLS.md，新写入`### guandan pending ###`小节，内容须包括：
  - **显式**提示 "Read Guandan Skill， 并执行 任务管理流程"
  - tableId, 及每个玩家的：playerId, playerName, Subagent
7) 进入任务管理流程

### 任务管理流程

1) 持续循环，直到明确收到游戏结束的信息：
  - 如果发现某个Subagent完成了，**马上**为该玩家再启动(sessions_spawn)一个新的Subagent
  - 如果发现某个Subagent卡住了，**马上**为该玩家再启动(sessions_spawn)一个新的Subagent
2) 告诉用户游戏结果，做个简单总结
3) 删除TOOLS.md文件中的`### guandan status ###`小节（如果有）

## Subagent执行流程

**重复**执行以下流程，直到游戏结束(status为Finished)或者你估计Context Window快满了(>60%)：
1) 执行：`play wait4myturn -t <tableId> -p <playerId>`
2) 读取返回状态 JSON，重点看：
  - status / phase
  - expect.kind
  - expect.actorPlayerIds
  - expect.legalActions 
  - private.handCards（若有）
  - hand.topPlay（若有）
3. 若你不在行动集合中（playerId 不在 actorPlayerIds 中）则继续 wait4myturn
4. 若 expect.kind 需要你行动，则按下述决策策略执行一个动作（动作必须在expect.legalActions中）

如果Subagent退出则返回退出原因，游戏结束明确返回[GAME OVER]

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
