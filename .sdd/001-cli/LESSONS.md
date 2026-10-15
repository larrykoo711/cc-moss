# LESSONS — 001-cli

## L1 playground 模板没有表格样式

- 现象：照模板填入 `<table>` 后渲染成无边框、无表头底色的裸表格
- 根因：sdd 的 playground 模板只为 Kanban、chips、fold 等组件写了样式，没有通用表格样式
- 规避：本档案在 `<style>` 末尾补了 `main table/th/td`；S6 自我升级时把它回补进 skill 模板

## L2 playground 的 `<pre>` 里不能出现 `-->`

- 现象：`gen_kanban.py --check` 报「HTML 注释开闭不配平」
- 根因：ASCII 箭头 `-->` 与 HTML 注释结束符相同，检查脚本按字面计数
- 规避：箭头一律写 `─→` 或 `→`；命令行参数里的 `--` 写成 `&#45;&#45;`
