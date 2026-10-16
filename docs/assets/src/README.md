# Picker screenshots

The picker images in `docs/assets/` are rendered from real picker screens, captured with
`tmux capture-pane -e -p` from a demo registry (made-up projects such as acme and billing), one
capture per language:

| Capture | Image |
|---------|-------|
| `main.ansi`, `main.zh-CN.ansi` | `picker.svg`, `picker.zh-CN.svg` |
| `interrupted.ansi`, `interrupted.zh-CN.ansi` | `picker-restore.svg`, `picker-restore.zh-CN.svg` |
| `menu.ansi`, `menu.zh-CN.ansi` | `picker-menu.svg`, `picker-menu.zh-CN.svg` |

Render one with:

```sh
python3 docs/assets/src/ansi2svg.py docs/assets/src/main.ansi docs/assets/picker.svg "t — cc-moss"
```

When the picker's wording changes, update the captures (or re-capture them) and re-render, so the
README never shows text the program no longer prints.
