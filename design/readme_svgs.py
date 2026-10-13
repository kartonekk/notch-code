"""Animated mockups for the README (docs/*.svg).

GitHub shows SVGs in <img> tags without scripts, so everything moves with SMIL
(<animate>, one loop per picture) and a few CSS keyframes. Colours, sizes and
radii mirror app/src/notch.css: a black island glued to the top edge, near-black
cards with a coloured glow at the bottom, white and grey pill buttons.

    python design/readme_svgs.py
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs"

INK = "#f5f6f8"
TEXT = "#c8ccd2"
DIM = "#9398a1"
DIM2 = "#8e939c"
DIM3 = "#6b7079"
DIM4 = "#5f646d"
CARD = "#141518"
RED, RED_TEXT = "#f4505e", "#ff8d97"
GREEN, GREEN2 = "#22c55e", "#34d399"
AMBER, AMBER_TEXT = "#f5a524", "#f7c25c"
SKY, CYAN, INDIGO, CLAUDE = "#38bdf8", "#22d3ee", "#6366f1", "#d97757"

CSS = """
text { font-family: "Segoe UI Variable Text", "Segoe UI", -apple-system, BlinkMacSystemFont, "Helvetica Neue", Arial, sans-serif; }
.mono { font-family: "Cascadia Mono", Consolas, ui-monospace, monospace; }
.spin { animation: spin 0.9s linear infinite; transform-box: fill-box; transform-origin: center; }
.breathe { animation: breathe 4s ease-in-out infinite; }
.blink { animation: breathe 1.4s ease-in-out infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
@keyframes breathe { 50% { opacity: 0.4; } }
@media (prefers-reduced-motion: reduce) { * { animation: none !important; } }
"""

_defs: list[str] = []


def svg(w, h, body):
    defs = "".join(_defs)
    _defs.clear()
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<style>{CSS}</style>
<defs>{defs}</defs>
{body}
</svg>
"""


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def text(x, y, s, size=12.5, fill=INK, weight=400, anchor="start", extra=""):
    return f'<text x="{x}" y="{y}" font-size="{size}" fill="{fill}" font-weight="{weight}" text-anchor="{anchor}" {extra}>{esc(s)}</text>'


def hsl(h, s, l, a=1):
    return f"hsla({h}, {s}%, {l}%, {a})"


_ids = iter(range(10_000))


def uid(prefix):
    return f"{prefix}{next(_ids)}"


# --------------------------------------------------------------------------- building blocks


def desktop(w, h, windows=True):
    g = uid("desk")
    _defs.append(
        f'<linearGradient id="{g}" x1="0" y1="0" x2="0.4" y2="1">'
        '<stop offset="0" stop-color="#1c1f2b"/><stop offset="0.6" stop-color="#14151c"/><stop offset="1" stop-color="#0d0e12"/></linearGradient>'
    )
    out = f'<rect width="{w}" height="{h}" rx="14" fill="url(#{g})"/>'
    if not windows:
        return out
    return out + (
        f'<rect x="{w * 0.06:.0f}" y="{h * 0.62:.0f}" width="{w * 0.42:.0f}" height="{h * 0.5:.0f}" rx="10" fill="#ffffff" opacity="0.035"/>'
        f'<rect x="{w * 0.56:.0f}" y="{h * 0.55:.0f}" width="{w * 0.36:.0f}" height="{h * 0.6:.0f}" rx="10" fill="#ffffff" opacity="0.025"/>'
    )


def island(cx, w, h, r, extra=""):
    x = cx - w / 2
    return (
        f'<path d="M{x} 0 H{x + w} V{h - r} A{r} {r} 0 0 1 {x + w - r} {h} H{x + r} A{r} {r} 0 0 1 {x} {h - r} Z" fill="#000" {extra}/>'
    )


def tone_edge(cx, w, h, r, color, opacity=0.75):
    """the coloured hairline + glow along the bottom of the compact island"""
    x = cx - w / 2
    g = uid("edge")
    _defs.append(
        f'<radialGradient id="{g}" cx="0.5" cy="1.7" r="0.6" gradientTransform="translate(0.5 0) scale(1 2.2) translate(-0.5 0)">'
        f'<stop offset="0" stop-color="{color}" stop-opacity="0.45"/><stop offset="1" stop-color="{color}" stop-opacity="0"/></radialGradient>'
    )
    return (
        f'<rect x="{x}" y="0" width="{w}" height="{h}" fill="url(#{g})"/>'
        f'<path d="M{x + r * 0.3} {h - 0.75} H{x + w - r * 0.3}" stroke="{color}" stroke-opacity="{opacity}" stroke-width="1.5"/>'
    )


def card(x, y, w, h, glow=None, r=20):
    out = f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{CARD}" stroke="#ffffff" stroke-opacity="0.035"/>'
    if glow:
        color, alpha = glow
        g, c = uid("wash"), uid("clip")
        _defs.append(
            f'<radialGradient id="{g}" cx="0.5" cy="1.35" r="0.65" gradientTransform="translate(0.5 1.35) scale(1 {min(2.2, w / h * 0.9):.2f}) translate(-0.5 -1.35)">'
            f'<stop offset="0" stop-color="{color}" stop-opacity="{alpha}"/><stop offset="1" stop-color="{color}" stop-opacity="0"/></radialGradient>'
            f'<clipPath id="{c}"><rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}"/></clipPath>'
        )
        out += f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="url(#{g})" clip-path="url(#{c})"/>'
    return out


def orb(cx, cy, color, ring=False, glow=True, cls=""):
    out = f'<circle cx="{cx}" cy="{cy}" r="4" fill="{color}" class="{cls}"/>'
    if glow:
        out = f'<circle cx="{cx}" cy="{cy}" r="6" fill="{color}" opacity="0.25"/>' + out
    if ring:
        out += (
            f'<circle cx="{cx}" cy="{cy}" r="8.5" fill="none" stroke="{color}" stroke-opacity="0.25" stroke-width="1.5"/>'
            f'<path class="spin" d="M{cx - 8.5} {cy} A8.5 8.5 0 0 1 {cx} {cy - 8.5}" fill="none" stroke="{color}" stroke-width="1.5" stroke-linecap="round"/>'
        )
    return out


def mini(x, y, hue, size=14, running=False):
    h = size * 0.82
    g = uid("mini")
    _defs.append(
        f'<linearGradient id="{g}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{hsl(hue, 85, 74)}"/><stop offset="1" stop-color="{hsl(hue, 70, 56)}"/></linearGradient>'
    )
    out = f'<rect x="{x}" y="{y - h / 2}" width="{size}" height="{h}" rx="{size * 0.34}" fill="url(#{g})"/>'
    if running:
        cx, r = x + size / 2, size / 2 + 3
        out += (
            f'<circle cx="{cx}" cy="{y}" r="{r}" fill="none" stroke="{hsl(hue, 80, 70, 0.25)}" stroke-width="1.5"/>'
            f'<path class="spin" d="M{cx - r} {y} A{r} {r} 0 0 1 {cx} {y - r}" fill="none" stroke="{hsl(hue, 85, 72)}" stroke-width="1.5" stroke-linecap="round"/>'
        )
    return out


def btn(x, y, label, primary=False, w=None, size=12.5, icon=None):
    w = w or len(label) * size * 0.56 + 26 + (14 if icon else 0)
    h = size + 15
    fill = INK if primary else "#ffffff"
    op = 1 if primary else 0.09
    color = "#0b0c0e" if primary else "#f1f2f4"
    out = f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{h / 2}" fill="{fill}" fill-opacity="{op}"/>'
    tx = x + w / 2 + (7 if icon else 0)
    if icon == "stop":
        out += f'<rect x="{x + 13}" y="{y + h / 2 - 4}" width="8" height="8" rx="2" fill="{color}"/>'
    elif icon == "rec":
        out += f'<circle cx="{x + 17}" cy="{y + h / 2}" r="4" fill="{RED}"/>'
    out += text(tx, y + h / 2 + size * 0.36, label, size, color, 500, "middle")
    return out, w


def shimmer_text(x, y, s, size=13, weight=500):
    g = uid("shim")
    _defs.append(
        f'<linearGradient id="{g}" gradientUnits="userSpaceOnUse" x1="{x - 200}" y1="0" x2="{x + 100}" y2="0">'
        '<stop offset="0" stop-color="#7c818a"/><stop offset="0.4" stop-color="#f2f3f5"/><stop offset="0.7" stop-color="#7c818a"/>'
        f'<animateTransform attributeName="gradientTransform" type="translate" from="0 0" to="{len(s) * size * 0.6 + 260:.0f} 0" dur="2.2s" repeatCount="indefinite"/>'
        "</linearGradient>"
    )
    return text(x, y, s, size, f"url(#{g})", weight)


def spinner(cx, cy, color, r=5):
    return (
        f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="#ffffff" stroke-opacity="0.15" stroke-width="1.5"/>'
        f'<path class="spin" d="M{cx - r} {cy} A{r} {r} 0 0 1 {cx} {cy - r}" fill="none" stroke="{color}" stroke-width="1.5" stroke-linecap="round"/>'
    )


ICONS = {
    "home": '<path fill="currentColor" stroke="none" d="M10.6 3.1a2.2 2.2 0 0 1 2.8 0l7 5.7c.4.3.6.8.6 1.3V19a2 2 0 0 1-2 2h-4v-5.5a1.5 1.5 0 0 0-1.5-1.5h-3A1.5 1.5 0 0 0 9 15.5V21H5a2 2 0 0 1-2-2v-8.9c0-.5.2-1 .6-1.3z"/>',
    "mic": '<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10a7 7 0 0 0 14 0M12 17v5"/>',
    "plus": '<path d="M12 5v14M5 12h14"/>',
    "pin": '<path d="M12 17v5M9 3h6l-1 7 4 3v2H6v-2l4-3-1-7z"/>',
    "power": '<path d="M12 3v9M6.3 6.3a8 8 0 1 0 11.4 0"/>',
    "read": '<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"/>',
    "search": '<path d="M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4"/>',
    "edit": '<path d="M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4"/>',
    "run": '<path d="M4 17l6-5-6-5M12 19h8"/>',
    "plan": '<path d="M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01"/>',
    "check": '<path d="M5 12l5 5 9-10"/>',
    "send": '<path d="M12 19V5M5 12l7-7 7 7"/>',
    "image": '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="M21 15l-5-5L5 21"/>',
    "terminal": '<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M6 9l3 3-3 3M12 15h5"/>',
    "chevron": '<path d="M6 9l6 6 6-6"/>',
    "folder": '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
}


def icon(name, x, y, size=13, color=DIM2):
    s = size / 24
    return (
        f'<g transform="translate({x} {y}) scale({s:.4f})" fill="none" stroke="{color}" color="{color}" '
        f'stroke-width="{2 / s * size / 13 * 0.9:.2f}" stroke-linecap="round" stroke-linejoin="round">{ICONS[name]}</g>'
    )


def header(L, W, tabs, right=True):
    """the tab row: tabs = [(kind, label, active, extra)]"""
    out, x, y = "", L + 14, 15
    for kind, label, active, extra in tabs:
        if kind == "sep":
            out += f'<rect x="{x + 2}" y="{y + 5}" width="1" height="12" fill="#fff" fill-opacity="0.1"/>'
            x += 10
            continue
        if kind == "icon":
            w = 30
            body = icon(label, x + 8.5, y + 4.5, 13, INK if active else DIM2)
        elif kind == "rec":
            w = 66
            body = f'<circle class="blink" cx="{x + 13}" cy="{y + 11}" r="3.5" fill="{RED_TEXT}"/>' + text(x + 22, y + 15, label, 11.5, RED_TEXT, 600)
        else:
            w = len(label) * 6.4 + 30
            color = hsl(extra, 85, 84) if active else DIM2
            body = mini(x + 8, y + 11, extra, 10) + text(x + 23, y + 15, label, 11.5, color, 500)
        fill = hsl(extra, 30, 15) if (active and kind == "session") else "#1d1f23"
        if active:
            out += f'<rect x="{x}" y="{y}" width="{w}" height="22" rx="11" fill="{fill}"/>'
        out += body
        x += w + 5
    if right:
        rx = L + W - 16
        for name in ("power", "pin", "mic"):
            rx -= 14
            out += icon(name, rx, y + 4, 14, DIM2)
            rx -= 10
        cw = 124
        rx -= cw
        out += f'<rect x="{rx}" y="{y + 1}" width="{cw}" height="20" rx="10" fill="#fff" fill-opacity="0.06"/>'
        out += text(rx + 9, y + 15, "5h", 11, DIM)
        out += f'<rect x="{rx + 25}" y="{y + 9.5}" width="34" height="3" rx="1.5" fill="#fff" fill-opacity="0.14"/><rect x="{rx + 25}" y="{y + 9.5}" width="{34 * 0.37:.1f}" height="3" rx="1.5" fill="{GREEN}"/>'
        out += text(rx + 65, y + 15, "37% used", 11, DIM)
    return out


# --------------------------------------------------------------------------- pictures


def notch():
    """The collapsed island cycling through what it shows."""
    W, H, cx = 900, 150, 450
    dur, n = 12, 4
    states = []

    # idle: just the orb and the sessions' colours
    w = 288
    states.append(
        island(cx, w, 32, 14)
        + orb(cx - w / 2 + 20, 16, DIM4, glow=False, cls="breathe")
        + "".join(mini(cx + w / 2 - 22 - i * 16, 16, h, 12) for i, h in enumerate((205, 32, 140)))
    )
    # recording a meeting
    w = 440
    bars = "".join(
        f'<rect x="{cx + w / 2 - 40 + i * 4.5}" y="{16 - 7 * a:.1f}" width="2.5" height="{14 * a:.1f}" rx="1.2" fill="{RED}">'
        f'<animate attributeName="height" values="{14 * a:.1f};14;{14 * a:.1f}" dur="{0.5 + i * 0.13:.2f}s" repeatCount="indefinite"/>'
        f'<animate attributeName="y" values="{16 - 7 * a:.1f};9;{16 - 7 * a:.1f}" dur="{0.5 + i * 0.13:.2f}s" repeatCount="indefinite"/></rect>'
        for i, a in enumerate((0.3, 0.6, 0.4, 0.8, 0.35))
    )
    states.append(
        island(cx, w, 32, 14)
        + tone_edge(cx, w, 32, 14, RED, 1)
        + orb(cx - w / 2 + 20, 16, RED)
        + text(cx - w / 2 + 34, 20.5, "12:04", 12.5, RED_TEXT, 600)
        + text(cx - w / 2 + 76, 20.5, "Others:", 12.5, DIM, 600)
        + text(cx - w / 2 + 124, 20.5, "so we ship it on Friday, right?", 12.5, "#e8e9ec")
        + bars
    )
    # Claude at work
    states.append(
        island(cx, w, 32, 14)
        + tone_edge(cx, w, 32, 14, SKY, 0.8)
        + orb(cx - w / 2 + 20, 16, SKY, ring=True)
        + shimmer_text(cx - w / 2 + 36, 20.5, "notch-code: Editing App.tsx", 12.5, 400)
        + text(cx + w / 2 - 16, 20.5, "00:42", 11.5, DIM, anchor="end")
    )
    # finished while closed
    states.append(
        island(cx, w, 32, 14)
        + tone_edge(cx, w, 32, 14, GREEN, 1)
        + orb(cx - w / 2 + 20, 16, GREEN)
        + text(cx - w / 2 + 36, 20.5, "notch-code · done in 1 min 12 s", 12.5, INK, 600)
        + text(cx + w / 2 - 16, 20.5, "hover to see", 11, DIM4, anchor="end")
    )

    s = 1 / n  # each state's share of the loop
    f = 0.25 / dur  # fade out, then fade in
    body = desktop(W, H, windows=False)
    for i, state in enumerate(states):
        a, b = i * s, (i + 1) * s
        if i == 0:
            keys, vals = [0, f, b - f, b, 1], [0, 1, 1, 0, 0]
        else:
            keys, vals = [0, a, a + f, b - f, b, 1], [0, 0, 1, 1, 0, 0]
        body += (
            f'<g opacity="{vals[0]}"><animate attributeName="opacity" dur="{dur}s" repeatCount="indefinite" '
            f'keyTimes="{";".join(f"{k:.4f}" for k in keys)}" values="{";".join(map(str, vals))}"/>{state}</g>'
        )
    labels = ("Nothing going on", "Recording a meeting", "Claude at work", "Done — hover to read")
    for i, lab in enumerate(labels):
        x = 150 + i * 200
        a, b = i * s, (i + 1) * s
        kt, vals = ("0;" + f"{b:.4f}", "1;0") if i == 0 else (f"0;{a:.4f};{b:.4f}", "0;1;0")
        body += f'<rect x="{x - 40}" y="128" width="80" height="2.5" rx="1.25" fill="#fff" fill-opacity="0.08"/>'
        body += (
            f'<rect x="{x - 40}" y="128" width="80" height="2.5" rx="1.25" fill="{INK}" opacity="0">'
            f'<animate attributeName="opacity" dur="{dur}s" repeatCount="indefinite" keyTimes="{kt}" values="{vals}" calcMode="discrete"/></rect>'
        )
        body += (
            f'<text x="{x}" y="118" font-size="12" fill="{DIM3}" font-weight="500" text-anchor="middle">{esc(lab)}'
            f'<animate attributeName="fill" dur="{dur}s" repeatCount="indefinite" keyTimes="{kt}" '
            f'values="{vals.replace("1", INK).replace("0", DIM3)}" calcMode="discrete"/></text>'
        )
    return svg(W, H, body)


def overview():
    """The open island: what's going on (left) and a pill per place to jump into (right)."""
    W, H, cx = 900, 230, 450
    w, h = 640, 184
    L = cx - w / 2
    body = desktop(W, H) + island(cx, w, h, 22)
    body += header(L, w, [("icon", "home", True, 0), ("icon", "mic", False, 0), ("sep", "", False, 0), ("session", "notch-code", False, 205), ("session", "api", False, 32), ("icon", "plus", False, 0)])

    # status card
    x, y, cw, ch = L + 10, 42, 322, 132
    body += card(x, y, cw, ch, (SKY, 0.22))
    body += f'<circle cx="{x + 20}" cy="{y + 21}" r="4" fill="{hsl(205, 85, 68)}"/>'
    body += text(x + 31, y + 25.5, "notch-code", 12.5, INK, 600) + text(x + 105, y + 25.5, "Claude Code", 11.5, DIM2)
    body += text(x + cw - 16, y + 25.5, "00:42", 11, DIM3, anchor="end")
    ry = y + 42
    body += '<g opacity="0.7">' + icon("search", x + 16, ry - 1, 12, DIM2) + text(x + 35, ry + 9.5, "Searched for setNotchSize", 13, DIM3, 500) + "</g>"
    ry += 22
    body += spinner(x + 22, ry + 5, SKY) + shimmer_text(x + 35, ry + 9.5, "Editing App.tsx")
    b1, bw = btn(x + 16, y + ch - 36, "Watch", True)
    b2, _ = btn(x + 16 + bw + 8, y + ch - 36, "Stop", icon="stop")
    body += b1 + b2

    # pills card
    px, pw = x + cw + 10, w - cw - 30
    body += card(px, y, pw, ch)
    pills = [
        ("rec", "Transcribe", None, False),
        ("session", "notch-code", 205, True),
        ("session", "api", 32, False),
        ("session", "website", 140, False),
        ("ghost", "dotfiles", 280, False),
        ("plus", "New session", None, False),
    ]
    colw = (pw - 26) / 2
    for i, (kind, label, hue, busy) in enumerate(pills):
        qx = px + 10 + (i % 2) * (colw + 6)
        qy = y + 22 + (i // 2) * 32
        if kind == "session":
            stroke = hsl(hue, 75, 62, 0.85) if busy else hsl(hue, 55, 45, 0.55)
            body += f'<rect x="{qx}" y="{qy}" width="{colw}" height="28" rx="14" fill="{hsl(hue, 40, 11)}" stroke="{stroke}"/>'
            body += mini(qx + 8, qy + 14, hue, 14, busy) + text(qx + colw / 2 + 8, qy + 18, label, 11, hsl(hue, 80, 76), 600, "middle")
        elif kind == "ghost":
            body += f'<rect x="{qx}" y="{qy}" width="{colw}" height="28" rx="14" fill="none" stroke="#fff" stroke-opacity="0.12" stroke-dasharray="3 3"/>'
            body += mini(qx + 8, qy + 14, hue, 14) + text(qx + colw / 2 + 8, qy + 18, label, 11, DIM3, 600, "middle")
        elif kind == "plus":
            body += f'<rect x="{qx}" y="{qy}" width="{colw}" height="28" rx="14" fill="none" stroke="#fff" stroke-opacity="0.12" stroke-dasharray="3 3"/>'
            body += f'<circle cx="{qx + 15}" cy="{qy + 14}" r="9" fill="#fff" fill-opacity="0.07"/>' + icon("plus", qx + 9.5, qy + 8.5, 11, DIM2)
            body += text(qx + colw / 2 + 8, qy + 18, label, 11, DIM3, 600, "middle")
        else:
            body += f'<rect x="{qx}" y="{qy}" width="{colw}" height="28" rx="14" fill="#0e0f11" stroke="#fff" stroke-opacity="0.14"/>'
            body += f'<circle cx="{qx + 15}" cy="{qy + 14}" r="9" fill="{RED}" fill-opacity="0.16"/>' + icon("mic", qx + 9.5, qy + 8.5, 11, RED_TEXT)
            body += text(qx + colw / 2 + 8, qy + 18, label, 11, DIM, 600, "middle")
    return svg(W, H, body)


def session():
    """A Claude session in Auto mode: the conversation, the step it's on, the chat bar."""
    W, H, cx = 900, 476, 450
    w, h = 640, 426
    L = cx - w / 2
    body = desktop(W, H) + island(cx, w, h, 22)
    body += header(L, w, [("icon", "home", False, 0), ("icon", "mic", False, 0), ("sep", "", False, 0), ("session", "notch-code", True, 205), ("session", "api", False, 32), ("icon", "plus", False, 0)])
    x, y, cw, ch = L + 10, 42, w - 20, h - 52
    body += card(x, y, cw, ch, (AMBER, 0.26))

    # card head
    body += f'<rect x="{x + 14}" y="{y + 13}" width="118" height="26" rx="13" fill="#fff" fill-opacity="0.07"/>'
    body += mini(x + 24, y + 26, 205, 10, True) + text(x + 41, y + 30.5, "notch-code", 12.5, INK, 600) + icon("chevron", x + 112, y + 21, 11, DIM2)
    body += text(x + 142, y + 30.5, "working", 11, DIM3)
    t1, tw = btn(x + cw - 200, y + 14, "Terminal", size=11.5, icon="terminal")
    t2, _ = btn(x + cw - 200 + tw + 6, y + 14, "New chat", size=11.5)
    body += t1 + icon("terminal", x + cw - 200 + 10, y + 19.5, 12, "#f1f2f4") + t2

    # feed
    fy = y + 60
    msg = "Make the notch look like the coucou island"
    mw = len(msg) * 6.6 + 22
    body += f'<rect x="{x + cw - 18 - mw}" y="{fy}" width="{mw}" height="28" rx="12" fill="#fff" fill-opacity="0.13"/>'
    body += text(x + cw - 18 - mw + 11, fy + 18.5, msg, 12.5, "#f1f2f4")
    fy += 44
    for line in ("I'll restyle the island to match: black cards with a soft glow,", "pill buttons, and a chat bar with a round send button."):
        body += text(x + 16, fy, line, 12.5, TEXT)
        fy += 19
    fy += 8
    steps = [("read", "Read 6 files", DIM3, "ok"), ("edit", "Editing notch.css", CYAN, "ok"), ("edit", "Editing App.tsx", CYAN, "ok"), ("run", "Running npm run build", AMBER, "run")]
    for ic, label, col, st in steps:
        body += icon(ic, x + 18, fy - 9, 12, col) + text(x + 37, fy + 0.5, label, 12.5, INK if st == "run" else "#b0b5be")
        if st == "ok":
            body += icon("check", x + cw - 34, fy - 9, 12, GREEN)
        else:
            body += text(x + cw - 50, fy + 0.5, "00:12", 10.5, "#9cd8fb", anchor="end") + spinner(x + cw - 28, fy - 3.5, SKY)
        fy += 24

    # run strip (Auto)
    sy = y + ch - 120
    body += f'<rect x="{x + 14}" y="{sy}" width="{cw - 28}" height="30" rx="12" fill="{AMBER}" fill-opacity="0.07" stroke="{AMBER}" stroke-opacity="0.2"/>'
    body += spinner(x + 30, sy + 15, AMBER)
    body += f'<rect x="{x + 42}" y="{sy + 8}" width="40" height="15" rx="7.5" fill="{AMBER}" fill-opacity="0.16"/>' + text(x + 62, sy + 19, "AUTO", 9.5, AMBER_TEXT, 700, "middle")
    body += shimmer_text(x + 92, sy + 19.5, "Running npm run build", 12, 500)
    body += text(x + cw - 100, sy + 19.5, "00:58", 11, DIM2, anchor="end")
    sb, _ = btn(x + cw - 92, sy + 4, "", size=11, w=72)
    body += sb + text(x + cw - 80, sy + 19.5, "Stop", 11.5, "#f1f2f4", 500)
    body += f'<rect x="{x + cw - 50}" y="{sy + 9}" width="24" height="15" rx="4" fill="#fff" fill-opacity="0.1"/>' + text(x + cw - 38, sy + 19.5, "Esc", 9.5, DIM, 600, "middle")

    # chat bar
    cy = sy + 38
    body += f'<rect x="{x + 14}" y="{cy}" width="{cw - 28}" height="42" rx="14" fill="#fff" fill-opacity="0.07"/>'
    body += text(x + 28, cy + 25.5, "Add more details — Claude reads it at its next step", 13, DIM3)
    body += icon("image", x + cw - 82, cy + 14, 13, DIM2)
    body += f'<circle cx="{x + cw - 35}" cy="{cy + 21}" r="14" fill="{INK}"/>' + icon("send", x + cw - 42, cy + 14, 14, "#0b0c0e")

    # foot: modes, hint, model
    fy = cy + 50
    body += f'<rect x="{x + 16}" y="{fy}" width="52" height="20" rx="10" fill="{AMBER}" fill-opacity="0.16"/>'
    body += f'<circle cx="{x + 27}" cy="{fy + 10}" r="3" fill="{AMBER}"/>' + text(x + 35, fy + 14, "Auto", 11, AMBER_TEXT)
    body += icon("edit", x + 79, fy + 4.5, 11, DIM3) + text(x + 94, fy + 14, "Edits", 11, DIM3)
    body += icon("plan", x + 133, fy + 4.5, 11, DIM3) + text(x + 148, fy + 14, "Plan", 11, DIM3)
    body += text(x + 186, fy + 14, "No brakes · changes kept automatically", 11, DIM4)
    body += f'<rect x="{x + cw - 190}" y="{fy + 2}" width="96" height="16" rx="8" fill="#fff" fill-opacity="0.06"/>'
    body += text(x + cw - 142, fy + 13.5, "CAVEMAN ULTRA", 8.5, DIM, 600, "middle", 'letter-spacing="0.6"')
    body += f'<rect x="{x + cw - 86}" y="{fy}" width="70" height="20" rx="10" fill="#fff" fill-opacity="0.06"/>'
    body += f'<circle cx="{x + cw - 75}" cy="{fy + 10}" r="3" fill="{CLAUDE}"/>' + text(x + cw - 67, fy + 14, "Opus", 11, "#9a9fa8", 500)
    body += icon("chevron", x + cw - 34, fy + 4.5, 11, "#7b8089")
    return svg(W, H, body)


def transcribe():
    """Live transcription: what's recorded, levels, timer, Pause / End, the transcript below."""
    W, H, cx = 900, 340, 450
    w, h = 640, 296
    L = cx - w / 2
    body = desktop(W, H) + island(cx, w, h, 22)
    body += header(L, w, [("icon", "home", False, 0), ("rec", "12:04", True, 0), ("sep", "", False, 0), ("session", "notch-code", False, 205), ("icon", "plus", False, 0)])
    x, y, cw, ch = L + 10, 42, w - 20, h - 52
    body += card(x, y, cw, ch, (RED, 0.24))

    # live head
    hy = y + 14
    body += f'<circle cx="{x + 22}" cy="{hy + 15}" r="7" fill="{RED}" opacity="0.25"><animate attributeName="r" values="6;9;6" dur="0.9s" repeatCount="indefinite"/></circle>'
    body += f'<circle cx="{x + 22}" cy="{hy + 15}" r="4" fill="{RED}"/>'
    body += text(x + 38, hy + 11, "Recording", 12.5, INK, 600) + text(x + 102, hy + 11, "· Microsoft Teams", 12.5, DIM)
    for i, (lab, lvl) in enumerate((("App", 0.7), ("Mic", 0.35))):
        mx = x + 38 + i * 100
        body += text(mx, hy + 30, lab, 11, DIM3)
        body += f'<rect x="{mx + 24}" y="{hy + 24.5}" width="56" height="4" rx="2" fill="#fff" fill-opacity="0.08"/>'
        body += (
            f'<rect x="{mx + 24}" y="{hy + 24.5}" width="{56 * lvl}" height="4" rx="2" fill="{GREEN}">'
            f'<animate attributeName="width" values="{56 * lvl:.0f};{56 * lvl * 0.4:.0f};{56 * min(1, lvl * 1.2):.0f};{56 * lvl:.0f}" dur="{1.1 + i * 0.4}s" repeatCount="indefinite"/></rect>'
        )
    body += text(x + cw - 196, hy + 20, "12:04", 15, RED_TEXT, 600, "end")
    body += icon("folder", x + cw - 186, hy + 8, 14, DIM2)
    p1, pw = btn(x + cw - 160, hy + 3, "Pause")
    p2, _ = btn(x + cw - 160 + pw + 8, hy + 3, "End", True, icon="stop")
    body += p1 + p2
    body += f'<rect x="{x + 14}" y="{hy + 44}" width="{cw - 28}" height="1" fill="#fff" fill-opacity="0.06"/>'

    # transcript, lines appearing one by one
    lines = [
        ("11:41", "others", "Okay, the release branch is cut."),
        ("11:46", "me", "Did the Vulkan build pass on the AMD box?"),
        ("11:52", "others", "Yes, both machines, no regressions."),
        ("11:57", "me", "Great — then the notch redesign goes in too."),
        ("12:01", "others", "So we ship it on Friday, right?"),
        ("12:04", "me", "Friday it is."),
    ]
    ty = hy + 68
    dur = 9
    for i, (ts, who, s) in enumerate(lines):
        t = 0.08 + i * 0.12
        color = "#67e8f9" if who == "me" else "#fcd34d"
        row = (
            text(x + 16, ty, ts, 11, DIM4)
            + text(x + 62, ty, "Me" if who == "me" else "Others", 12, color, 600)
            + text(x + 124, ty, s, 12.5, TEXT)
        )
        if i < 2:  # already there when the picture is still
            body += row
        else:
            body += (
                f'<g opacity="0"><animate attributeName="opacity" dur="{dur}s" repeatCount="indefinite" '
                f'keyTimes="0;{t:.3f};{t + 0.03:.3f};0.94;1" values="0;0;1;1;0"/>{row}</g>'
            )
        ty += 24
    return svg(W, H, body)


def main():
    OUT.mkdir(exist_ok=True)
    for name, fn in (("notch", notch), ("overview", overview), ("session", session), ("transcribe", transcribe)):
        (OUT / f"{name}.svg").write_text(fn(), encoding="utf-8")
        print(f"docs/{name}.svg")


if __name__ == "__main__":
    main()
