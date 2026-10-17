<p align="center">
  <img src="docs/assets/banner.zh-CN.svg" alt="cc-moss：文明会延续，对话也是。" width="100%">
</p>

<p align="center">
  <b>简体中文</b> · <a href="README.en.md">English</a>
</p>

<p align="center">
  <a href="https://github.com/larrykoo711/cc-moss/releases/latest"><img alt="最新版本" src="https://img.shields.io/github/v/release/larrykoo711/cc-moss?style=flat-square&color=FF5A3C&label=%E7%89%88%E6%9C%AC"></a>
  <img alt="macOS · Linux" src="https://img.shields.io/badge/macOS%20%C2%B7%20Linux-71717a?style=flat-square">
  <a href="LICENSE"><img alt="MIT" src="https://img.shields.io/badge/%E5%BC%80%E6%BA%90-MIT-3b82f6?style=flat-square"></a>
</p>

---

电脑突然重启，编辑器崩了，或者手一滑关掉了终端。

刚才开着的那几个 Claude Code 会话，一下子都不见了。哪个在改登录页、哪个在查 bug、哪个跑到一半在等你回话？只能一份份翻记录，慢慢回忆。

**cc-moss 是个小工具，专门对付这件事**。它在后台默默记下你的每一个 Claude Code 会话；出了意外，一条命令，全部回来，回到原来的目录，接上原来的对话。

<p align="center">
  <img src="docs/assets/picker.zh-CN.svg" alt="cc-moss 的会话面板" width="92%">
</p>

## 它能帮你做什么

- **什么都不用记**：你照常用 Claude Code，cc-moss 自己会把每个会话登记好。
- **出了意外，一键找回**：崩溃、重启之后运行 `cc-moss restore`，每个会话回到原处，对话从断开的地方接着聊。
- **所有会话，一眼看清**：敲一个 `t`，会话按项目排好，谁在干活、谁在等你、谁断开了，清清楚楚。
- **给会话留张便条**：写一句「在做什么、停在哪」，第二天打开就知道从哪继续。

不用背快捷键：打字就是搜索，方向键挑，回车进去。界面跟着你的系统语言走，中文、英文都有。

## 状态栏 App（Mac）

Mac 上还多一个住在菜单栏里的小 App，连终端都不用打开，抬眼就知道会话们过得怎么样。

<p align="center">
  <img src="docs/assets/app-panel.zh-CN.png" alt="状态栏 App 的会话面板" width="420">
</p>

- **图标会动**：有会话在忙、在等你，菜单栏图标就动起来；嫌它晃眼，可以让它安安静静，只在状态变了时换个样子。
- **一个面板，全部会话**：点图标，或在哪儿都连按两下 ⌃，会话按项目排好；回车就在你的终端里打开（系统终端、Ghostty 等都行），断了的点一下找回来；新建会话、写便条、给会话发句话，也都在这里。
- **有事才叫你**：会话需要你确认或者突然断了，它会告诉你。通知中心和小莫二选一，同一件事不会被说两遍。
- **小莫**：一颗住在桌面角落的小石头，用表情告诉你会话们怎么样了。安静、日常、积极三种性子任选，想的话还能让它轻响一声，或者把消息念给你听。
- **重启之后不慌**：电脑重启后，它第一时间告诉你「有 3 个会话断了」，点一下全部回来。

想细看？[使用手册里的状态栏 App 一章](docs/guide.md#状态栏-app)把快捷键和设置都讲清楚了。

## 三步装好

```sh
brew install larrykoo711/tap/cc-moss
cc-moss install
cc-moss doctor
```

`install` 会先把要改的每一处给你看，你同意了才动手。它只在你的配置里**追加**，不会覆盖你原来的设置，改过的文件都留了备份。缺什么工具（比如 tmux），它会告诉你，问过你再帮你装上。最后的 `doctor` 会逐项检查，哪里没弄好就告诉你怎么修。

Mac 上，`install` 还会问你要不要把状态栏 App 放进「应用程序」文件夹，点头它就出现在菜单栏里了；以后 `cc-moss update` 会让命令行和 App 一起升级。Linux 上是纯终端版，上面这些终端功能一样不少。

需要：一台 Mac 或 Linux 电脑，装好了 [Claude Code](https://code.claude.com/docs)。没有 Homebrew？也可以[下载安装包或从源码安装](docs/guide.md#安装)。

## 平时怎么用

记住这三条就够了：

| 你想… | 就这样 |
|---|---|
| 看看都有哪些会话 | 敲 `t` |
| 开一个新会话 | `cc-moss new 名字`，例如 `cc-moss new 登录页` |
| 意外之后全部找回 | `cc-moss restore` |

<table>
  <tr>
    <td width="50%"><img src="docs/assets/picker-restore.zh-CN.svg" alt="一个中断了的会话"></td>
    <td width="50%"><img src="docs/assets/picker-menu.zh-CN.svg" alt="会话菜单"></td>
  </tr>
  <tr>
    <td align="center"><sub>断开的会话还留着你的便条和最后一句话，回车就能接着聊</sub></td>
    <td align="center"><sub>按 → 打开菜单：进入、写便条、结束会话</sub></td>
  </tr>
</table>

有新版本时，面板最上面会提醒你，选中回车就能升级。

## 你的数据

所有记录只存在你自己的电脑上，不上传，也没有任何统计。唯一的联网，是每天最多一次去 GitHub 问一句「有没有新版本」，不带任何关于你的信息，[随时可以关掉](docs/guide.md#隐私)。

## 不想用了

```sh
cc-moss install --uninstall
brew uninstall cc-moss
```

第一步把它加过的东西全部拿掉（Mac 上连状态栏 App 一起），改过的文件恢复原样；第二步再删掉程序本身。

## 想了解更多

- [使用手册](docs/guide.md)：全部命令、配置项、工作原理，以及它和其他做法有什么不同
- [状态栏 App](docs/guide.md#状态栏-app)：面板快捷键、通知、小莫和它的声音
- [常见问题](docs/guide.md#常见问题)：装完敲 `t` 没反应？用的是 bash？这里都有
- [更新记录](CHANGELOG.md)
- 想一起改进它？欢迎看看[参与指南](CONTRIBUTING.zh-CN.md)

## 为什么叫 MOSS？

MOSS 是《流浪地球》里的人工智能，它的使命是让人类文明延续下去。cc-moss 的使命小得多：不管你的电脑遇到了什么，让你和 Claude 的对话延续下去。

---

<sub>[MIT](LICENSE) 开源。cc-moss 是个人独立项目，与 Anthropic 没有关联。</sub>
