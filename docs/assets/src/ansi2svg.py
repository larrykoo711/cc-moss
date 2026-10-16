"""Render a `tmux capture-pane -e` dump as a terminal-window SVG."""
import html
import re
import sys
import unicodedata

FONT = 14
CW = FONT * 0.6  # cell width of a monospace font
LH = 20  # line height
PAD_X, PAD_TOP, PAD_BOTTOM, BAR = 22, 16, 18, 38
FG = "#e4e4e7"
BG = "#18181b"


def width(ch):
    return 2 if unicodedata.east_asian_width(ch) in "WF" else 1


def parse(text):
    """Yield rows of (col, text, fg, bg, bold) runs."""
    rows = []
    fg, bg, bold = None, None, False
    for line in text.rstrip("\n").split("\n"):
        runs, col = [], 0
        for part in re.split(r"(\x1b\[[0-9;:]*m)", line):
            m = re.fullmatch(r"\x1b\[([0-9;:]*)m", part)
            if m:
                codes = [int(c or 0) for c in re.split("[;:]", m.group(1))] or [0]
                i = 0
                while i < len(codes):
                    c = codes[i]
                    if c == 0:
                        fg, bg, bold = None, None, False
                    elif c == 1:
                        bold = True
                    elif c == 22:
                        bold = False
                    elif c == 39:
                        fg = None
                    elif c == 49:
                        bg = None
                    elif c in (38, 48) and codes[i + 1 : i + 2] == [2]:
                        rgb = "#%02x%02x%02x" % tuple(codes[i + 2 : i + 5])
                        fg, bg = (rgb, bg) if c == 38 else (fg, rgb)
                        i += 4
                    i += 1
                continue
            if part:
                runs.append((col, part, fg, bg, bold))
                col += sum(width(ch) for ch in part)
        rows.append(runs)
    return rows


def render(rows, cols, title):
    w = PAD_X * 2 + cols * CW
    h = BAR + PAD_TOP + len(rows) * LH + PAD_BOTTOM
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w + 40:.0f}" height="{h + 48:.0f}" '
        f'viewBox="0 0 {w + 40:.0f} {h + 48:.0f}" role="img" aria-label="{html.escape(title)}">',
        "<defs><filter id=\"s\" x=\"-10%\" y=\"-10%\" width=\"120%\" height=\"130%\">"
        "<feDropShadow dx=\"0\" dy=\"10\" stdDeviation=\"10\" flood-color=\"#000\" flood-opacity=\".35\"/>"
        "</filter></defs>",
        '<g transform="translate(20 14)">',
        f'<rect width="{w:.0f}" height="{h:.0f}" rx="12" fill="{BG}" filter="url(#s)"/>',
        f'<rect width="{w:.0f}" height="{h:.0f}" rx="12" fill="none" stroke="#3f3f46"/>',
        f'<path d="M0 {BAR} H{w:.0f}" stroke="#27272a"/>',
    ]
    for i, color in enumerate(("#ff5f57", "#febc2e", "#28c840")):
        out.append(f'<circle cx="{22 + i * 20}" cy="{BAR / 2}" r="6" fill="{color}"/>')
    out.append(
        f'<text x="{w / 2:.0f}" y="{BAR / 2 + 4.5}" fill="#a1a1aa" font-size="12.5" '
        'font-family="-apple-system,Segoe UI,Helvetica,Arial,sans-serif" '
        f'text-anchor="middle">{html.escape(title)}</text>'
    )
    out.append(
        '<g font-family="ui-monospace,SFMono-Regular,Menlo,Consolas,\'DejaVu Sans Mono\',monospace" '
        f'font-size="{FONT}" xml:space="preserve">'
    )
    for r, runs in enumerate(rows):
        y = BAR + PAD_TOP + r * LH
        for col, text, fg, bg, bold in runs:
            x = PAD_X + col * CW
            span = sum(width(ch) for ch in text) * CW
            if bg:
                out.append(f'<rect x="{x:.1f}" y="{y:.0f}" width="{span:.1f}" height="{LH}" fill="{bg}"/>')
            # Vertical rules must join across rows: draw them as shapes.
            if "│" in text and set(text) <= {"│", " "}:
                for i, ch in enumerate(text):
                    if ch == "│":
                        cx = x + (i + 0.5) * CW
                        out.append(f'<path d="M{cx:.1f} {y:.0f} v{LH}" stroke="{fg or "#5f5f5f"}"/>')
                continue
            if not text.strip():
                continue
            # Braille: draw the dots, fonts render them too small.
            is_braille = lambda ch: 0x2800 <= ord(ch) <= 0x28FF
            if any(is_braille(ch) for ch in text):
                col_off = 0
                plain = ""
                plain_start = 0
                cells = [(0, 0, 1), (0, 1, 2), (0, 2, 4), (1, 0, 8), (1, 1, 16), (1, 2, 32), (0, 3, 64), (1, 3, 128)]
                def flush(seg, start):
                    for m in re.finditer(r"\S+(?: \S+)*", seg):
                        sx = x + (start + sum(width(c) for c in seg[: m.start()])) * CW
                        sw = sum(width(c) for c in m.group(0)) * CW
                        out.append(
                            f'<text x="{sx:.1f}" y="{y + LH * 0.72:.1f}" fill="{fg or FG}"{" font-weight=\"700\"" if bold else ""} '
                            f'textLength="{sw:.1f}" lengthAdjust="spacingAndGlyphs">{html.escape(m.group(0))}</text>'
                        )
                for ch in text:
                    if is_braille(ch):
                        flush(plain, plain_start)
                        plain = ""
                        bits = ord(ch) - 0x2800
                        bx = x + col_off * CW
                        for cx, cy, b in cells:
                            if bits & b:
                                out.append(
                                    f'<circle cx="{bx + 2.3 + cx * 3.8:.1f}" cy="{y + 4 + cy * 3.9:.1f}" r="1.35" fill="{fg or FG}"/>'
                                )
                        col_off += 1
                        plain_start = col_off
                    else:
                        plain += ch
                        col_off += width(ch)
                flush(plain, plain_start)
                continue
            if set(text) <= {"─"}:
                out.append(
                    f'<path d="M{x:.1f} {y + LH / 2:.0f} h{span:.1f}" stroke="{fg or "#5f5f5f"}"/>'
                )
                continue
            weight = ' font-weight="700"' if bold else ""
            # Place each word group on its own cell so fonts cannot drift.
            for m in re.finditer(r"\S+(?: \S+)*", text):
                seg = m.group(0)
                sx = x + sum(width(ch) for ch in text[: m.start()]) * CW
                sw = sum(width(ch) for ch in seg) * CW
                out.append(
                    f'<text x="{sx:.1f}" y="{y + LH * 0.72:.1f}" fill="{fg or FG}"{weight} '
                    f'textLength="{sw:.1f}" lengthAdjust="spacingAndGlyphs">{html.escape(seg)}</text>'
                )
    out.append("</g></g></svg>")
    return "\n".join(out)


if __name__ == "__main__":
    src, dst, title = sys.argv[1:4]
    rows = parse(open(src, encoding="utf-8").read())
    while rows and not any(t.strip() for _, t, *_ in rows[-1]):
        rows.pop()
    cols = max((c + sum(width(ch) for ch in t) for row in rows for c, t, *_ in row), default=80)
    open(dst, "w", encoding="utf-8").write(render(rows, cols, title))
