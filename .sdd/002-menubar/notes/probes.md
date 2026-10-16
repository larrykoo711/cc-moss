# 探针结论（Q5、Q6）

核实日期：2026-10-16。环境：macOS 26.6，Swift 6.4（命令行工具，`xcode-select -p` = CommandLineTools）。
探针代码在会话临时目录，未入仓。

## Q5：只用命令行工具能否构建 SwiftUI App 并跑测试

- `swift build`：✅ 含 `MenuBarExtra` 的 SwiftUI 可执行目标可构建（debug 26 秒，release 22 秒，冷缓存）
- `swift test`：
  - **XCTest ❌**：`unable to resolve module dependency: 'XCTest'`（命令行工具不带 XCTest）
  - **Swift Testing ✅**：`import Testing` + `@Test` / `#expect` 正常运行
- 结论：测试一律用 Swift Testing，不用 XCTest；不要求开发者切到 Xcode。CI 的 macOS 镜像自带 Xcode，两者都可用

## Q6：ad-hoc 签名的 App 能否申请通知权限

做法：SwiftPM 构建 → 手工组装 `.app`（`Info.plist` 含 `CFBundleIdentifier`、`LSUIElement`）→ `codesign -s -` →
`open` 启动 → 在 `applicationDidFinishLaunching` 里 `UNUserNotificationCenter.requestAuthorization`。

| App 所在位置 | 结果 |
|--------------|------|
| 会话临时目录（`/private/tmp/...`），直接执行可执行文件 | ❌ `UNErrorDomain Code=1 "Notifications are not allowed for this application"` |
| 同上，经 `open`（LaunchServices）启动 | ❌ 同上 |
| 同上，换一个全新的 Bundle ID | ❌ 同上（排除「之前拒过被记住」） |
| `~/Applications/`，经 `open` 启动 | ✅ `auth true`，通知成功送达（全新 ID 与正式 ID `io.github.larrykoo711.cc-moss` 都成功） |

- 结论：**ad-hoc 签名可以用通知中心，但 App 必须放在「应用程序」目录（`/Applications` 或 `~/Applications`）里运行**。
  这与 A12 的分发方式吻合（`cc-moss install` 把 App 放进 `~/Applications`）
- 开发时同理：调试通知要先把构建产物装进 `~/Applications` 再 `open`；`apps/macos/scripts/dev-install.sh` 负责这件事
- 副作用：系统设置 →「通知」里会出现 cc-moss（正式 ID）与一个探针 ID 的条目；探针 App 已删除
