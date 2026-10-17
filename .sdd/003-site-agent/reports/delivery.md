# 003 交付报告：网站的搜索、AI 问答与 Agent 能力

收尾日期：2026-10-17。合入：#24（agent-ready site on Cloudflare Workers），之后的中文页图标修复随本次收尾一起合入。

## 交付指标（真实输出）

| 检查 | 结果 |
|------|------|
| `pnpm test`（site） | 32/32：协商、MCP、搜索、发布订阅源解析、构建产物一致性、链接可达 |
| `scripts/smoke.mjs https://cc-moss.larrykoo.com` | 46/46（页面、Markdown 协商、AI 抓取、406、重定向、404、发现文件、MCP 三工具） |
| isitagentready | Level 4「Agent-Integrated」 |
| is-agentic | 100 |
| seo-geo 审计 | 标题 53 / 38 字、描述 158 / 89 字、单 H1、JSON-LD、加载 0.50 / 0.69 秒 |

## 四桶

### 1. 完成（7）

S1–S5 与反馈车道 F1、F2，见 PLAN.md Kanban。

### 2. 新独立任务（提议）

| 项 | 建议去处 |
|----|----------|
| 在 Google Search Console 与 Bing Webmaster Tools 提交 sitemap、监控收录 | 需要站长账号，由用户操作；之后可开一张「搜索可见度」卡按月看收录与 AI 引用 |
| DNS-AID（`_mcp._agents` 记录） | 先确认 zone 是否开 DNSSEC，再决定 |

### 3. 新增子任务

| 项 | 建议 |
|----|------|
| 文档或站点改动后自动通知 IndexNow（Bing） | 在 Site 工作流部署成功后加一步 |

### 4. 延后决策

| 项 | 理由 |
|----|------|
| is-agentic 的 trust-anchors（独立的关于 / 联系 / 隐私页面） | 与「站点只放一页和 FAQ」的用户定调冲突；隐私说明已在 FAQ 与手册里 |
| Organization 结构化数据里的联系邮箱与地址 | 我们没有公开这些信息，不编造 |
| API 目录、OAuth、auth.md、A2A | 没有对应能力，按「不声明不存在的东西」不做 |

## 外部改动（记录在案）

- larrykoo.com 整个 zone 关闭了托管 robots.txt 与 AI 爬虫拦截（用户批准的「各站自管」）；同 zone 的其他站点
  自己的 robots.txt 本就放行 AI 爬虫
- 托管从 GitHub Pages 迁到 Cloudflare Worker `cc-moss-site`；仓库新增部署密钥两项；GitHub Pages 保留只为让旧
  github.io 地址继续跳转
