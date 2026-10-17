# 003: feature — 网站：搜索、AI 问答与 Agent

## 来源
- 无 tracker，序号 003（见 `.sdd/config.md` `{EDITION}`）
- 触发：2026-10-17 用户要求参照两个已在生产的兄弟站点（一个营销站、一个文档站）与 seo / geo / agent-readable skill 全面升级网站，含 MCP 能力

## 目录索引
| 文件 | 用途 |
|------|------|
| `PLAN.md` ⭐ | 决策、五个阶段、Kanban、评分结果、日志 |
| `reports/delivery.md` ⭐ | 收尾交付报告：指标、四桶、外部改动 |

## 状态
- ✅ 2026-10-17 收尾：网站由 Cloudflare Worker 提供（`site/worker/`），`/mcp` 可用；给人看的是首页与 FAQ，
  完整手册只以 Markdown 提供给 Agent
