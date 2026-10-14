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
GEMINI_TEXT = "#a5b4fc"

CSS = """
text { font-family: "Segoe UI Variable Text", "Segoe UI", -apple-system, BlinkMacSystemFont, "Helvetica Neue", Arial, sans-serif; }
.mono { font-family: "Cascadia Mono", Consolas, ui-monospace, monospace; }
.spin { animation: spin 0.9s linear infinite; transform-box: fill-box; transform-origin: center; }
.breathe { animation: breathe 4s ease-in-out infinite; }
.blink { animation: breathe 1.4s ease-in-out infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
@keyframes breathe { 50% { opacity: 0.4; } }
.m-blink { animation: m-blink 5s infinite; transform-box: fill-box; transform-origin: center; }
.m-eq { animation: m-eq 0.9s ease-in-out infinite; transform-box: fill-box; transform-origin: center; }
.m-eq.d2 { animation-delay: -0.3s; }
.m-eq.d4 { animation-delay: -0.6s; }
@keyframes m-blink { 0%, 93%, 100% { transform: scaleY(1); } 96% { transform: scaleY(0.08); } }
@keyframes m-eq { 0%, 100% { transform: scaleY(0.45); } 50% { transform: scaleY(1); } }
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
    "gear": '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>',
    "prev": '<path fill="currentColor" stroke="none" d="M19 5.5v13a1 1 0 0 1-1.5.9l-10-6.5a1 1 0 0 1 0-1.7l10-6.5a1 1 0 0 1 1.5.8zM4 5h2v14H4z"/>',
    "next": '<path fill="currentColor" stroke="none" d="M5 5.5v13a1 1 0 0 0 1.5.9l10-6.5a1 1 0 0 0 0-1.7l-10-6.5A1 1 0 0 0 5 5.5zM18 5h2v14h-2z"/>',
    "pause": '<path fill="currentColor" stroke="none" d="M7 5h3.5v14H7zM13.5 5H17v14h-3.5z"/>',
    "folder": '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
}


def icon(name, x, y, size=13, color=DIM2):
    s = size / 24
    return (
        f'<g transform="translate({x} {y}) scale({s:.4f})" fill="none" stroke="{color}" color="{color}" '
        f'stroke-width="{2 / s * size / 13 * 0.9:.2f}" stroke-linecap="round" stroke-linejoin="round">{ICONS[name]}</g>'
    )


def agent_tag(x, cy, provider):
    """the little C / G square that says which agent a session runs"""
    bg, color, letter = ("rgba(99, 102, 241, 0.22)", GEMINI_TEXT, "G") if provider == "gemini" else ("rgba(217, 119, 87, 0.18)", CLAUDE, "C")
    return f'<rect x="{x}" y="{cy - 6.5}" width="13" height="13" rx="4" fill="{bg}"/>' + text(x + 6.5, cy + 3, letter, 8.5, color, 700, "middle")


def notchy(x, y, w, accent, tile=False):
    """Notchy, the default mascot (app/src/Mascot.tsx): its 240 × 220 art scaled to w wide.
    tile=True puts it in its little screen tile, as in the overview's status card."""
    s = w / 240
    out = ""
    if tile:
        h = w * 220 / 240
        g, a = uid("tile"), uid("tacc")
        _defs.append(
            f'<linearGradient id="{g}" x1="0.3" y1="0" x2="0.7" y2="1"><stop offset="0" stop-color="#2b3042"/><stop offset="0.55" stop-color="#191b25"/><stop offset="1" stop-color="#0e0f14"/></linearGradient>'
            f'<radialGradient id="{a}" cx="0.5" cy="0" r="0.6"><stop offset="0" stop-color="{accent}" stop-opacity="0.18"/><stop offset="1" stop-color="{accent}" stop-opacity="0"/></radialGradient>'
        )
        out += (
            f'<rect x="{x}" y="{y}" width="{w}" height="{h:.1f}" rx="12" fill="url(#{g})" stroke="#fff" stroke-opacity="0.06"/>'
            f'<rect x="{x}" y="{y}" width="{w}" height="{h:.1f}" rx="12" fill="url(#{a})"/>'
            f'<path d="M{x + 12} {y + 0.5} H{x + w - 12}" stroke="#fff" stroke-opacity="0.12"/>'
        )

    def kick(lx, fx, values):
        return (
            f'<g><path d="M{lx} 88 V124" stroke="#000" stroke-width="7" stroke-linecap="round"/><ellipse cx="{fx}" cy="128" rx="10" ry="6" fill="{accent}"/>'
            f'<animateTransform attributeName="transform" type="rotate" values="{values}" dur="1.6s" repeatCount="indefinite" calcMode="spline" keySplines="0.42 0 0.58 1;0.42 0 0.58 1"/></g>'
        )

    art = (
        kick(100, 96, "-12 100 88;14 100 88;-12 100 88")
        + kick(140, 144, "14 140 88;-12 140 88;14 140 88")
        + '<path d="M40 0 H200 V56 A36 36 0 0 1 164 92 H76 A36 36 0 0 1 40 56 Z" fill="#000" stroke="#fff" stroke-opacity="0.08"/>'
        + f'<path d="M72 97 H168" stroke="{accent}" stroke-width="3" stroke-linecap="round"><animate attributeName="opacity" values="0.55;1;0.55" dur="2s" repeatCount="indefinite"/></path>'
        + '<g class="m-blink"><rect x="86" y="26" width="17" height="26" rx="8.5" fill="#f5f6f8"/><rect x="137" y="26" width="17" height="26" rx="8.5" fill="#f5f6f8"/></g>'
        + f'<circle cx="74" cy="60" r="7" fill="{accent}" opacity="0.45"/><circle cx="166" cy="60" r="7" fill="{accent}" opacity="0.45"/>'
        + '<rect x="107" y="60" width="6" height="16" rx="3" fill="#f5f6f8" class="m-eq"/>'
        + '<rect x="117" y="56" width="6" height="24" rx="3" fill="#f5f6f8" class="m-eq d2"/>'
        + '<rect x="127" y="60" width="6" height="16" rx="3" fill="#f5f6f8" class="m-eq d4"/>'
    )
    sway = f'<g><animateTransform attributeName="transform" type="rotate" values="-3 120 0;3 120 0;-3 120 0" dur="4.5s" repeatCount="indefinite" calcMode="spline" keySplines="0.42 0 0.58 1;0.42 0 0.58 1"/>{art}</g>'
    return out + f'<g transform="translate({x} {y}) scale({s:.4f})">{sway}</g>'


def header(L, W, tabs, right=True):
    """the tab row: tabs = [(kind, label, active, extra)]"""
    out, x, y = "", L + 14, 15
    for kind, label, active, extra, *rest in tabs:
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
            w = len(label) * 6.4 + 48
            color = hsl(extra, 85, 84) if active else DIM2
            body = mini(x + 8, y + 11, extra, 10) + text(x + 23, y + 15, label, 11.5, color, 500)
            body += agent_tag(x + w - 22, y + 11, rest[0] if rest else "claude")
        fill = hsl(extra, 30, 15) if (active and kind == "session") else "#1d1f23"
        if active:
            out += f'<rect x="{x}" y="{y}" width="{w}" height="22" rx="11" fill="{fill}"/>'
        out += body
        x += w + 5
    if right:
        rx = L + W - 16
        for name in ("power", "pin", "gear", "mic"):
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


# --------------------------------------------------------------------------- timing


def shown(dur, t0, t1=1.0, fade=0.0):
    """an <animate> that shows its parent from t0 to t1 (fractions of a dur-second loop)"""
    if fade:
        a, b = max(0.0, t0 - fade), max(t0, t1 - fade)
        keys, vals = ([0, a, t0, b, t1, 1], [0, 0, 1, 1, 0, 0]) if t1 < 1 else ([0, a, t0, 1], [0, 0, 1, 1])
        if t0 == 0:
            keys, vals = ([0, b, t1, 1], [1, 1, 0, 0]) if t1 < 1 else ([0, 1], [1, 1])
        mode = ""
    else:
        keys, vals = ([0, t0, t1], [0, 1, 0]) if t1 < 1 else ([0, t0], [0, 1])
        if t0 == 0:
            keys, vals = ([0, t1], [1, 0]) if t1 < 1 else ([0], [1])
        mode = ' calcMode="discrete"'
    return (
        f'<animate attributeName="opacity" dur="{dur}s" repeatCount="indefinite"{mode} '
        f'keyTimes="{";".join(f"{k:.4f}" for k in keys)}" values="{";".join(map(str, vals))}"/>'
    )


def during(dur, t0, t1, body, fade=0.0):
    return f'<g opacity="{1 if t0 == 0 else 0}">{shown(dur, t0, t1, fade)}{body}</g>'


def steps_of(dur, items):
    """items = [(t0, body)]: each body is on screen from its t0 until the next one's"""
    out = ""
    for i, (t0, body) in enumerate(items):
        if body:
            out += during(dur, t0, items[i + 1][0] if i + 1 < len(items) else 1.0, body)
    return out


def typed(x, y, s, dur, t0, t1, size=13, fill=INK, hide=0.97):
    """text typed out letter by letter between t0 and t1, gone again at hide"""
    c = uid("type")
    w = len(s) * size * 0.52 + 6
    _defs.append(
        f'<clipPath id="{c}"><rect x="{x - 2}" y="{y - size - 4}" width="0" height="{size + 10}">'
        f'<animate attributeName="width" dur="{dur}s" repeatCount="indefinite" keyTimes="0;{t0:.4f};{t1:.4f};1" values="0;0;{w:.0f};{w:.0f}"/></rect></clipPath>'
    )
    caret = (
        f'<rect x="{x}" y="{y - size + 1}" width="1.5" height="{size + 2}" fill="{INK}">'
        f'<animate attributeName="x" dur="{dur}s" repeatCount="indefinite" keyTimes="0;{t0:.4f};{t1:.4f};1" values="{x};{x};{x + w - 5:.0f};{x + w - 5:.0f}"/></rect>'
    )
    return during(dur, t0, hide, f'<g clip-path="url(#{c})">{text(x, y, s, size, fill)}</g>' + f'<g class="blink">{caret}</g>')


def mascot_drop(cx, top, dur, t_in, t_out, accent):
    """Notchy dripping out from behind the island's lower edge (stretch, wobble), then pulled back up"""
    land = t_in + 0.035

    def k(v):
        return f"{v:.4f}"

    drop = (
        f'<animateTransform attributeName="transform" type="translate" dur="{dur}s" repeatCount="indefinite" '
        f'keyTimes="0;{k(t_in)};{k(land)};{k(t_out - 0.02)};{k(t_out)};1" values="0 -72;0 -72;0 0;0 0;0 -72;0 -72"/>'
    )
    squash = (
        f'<animateTransform attributeName="transform" type="scale" dur="{dur}s" repeatCount="indefinite" '
        f'keyTimes="0;{k(t_in)};{k(land)};{k(land + 0.012)};{k(land + 0.024)};{k(land + 0.036)};1" '
        'values="1 1;1 1;0.86 1.22;1.08 0.9;0.97 1.04;1 1;1 1"/>'
    )
    return f'<g>{drop}<g transform="translate({cx} {top})"><g>{squash}<g transform="translate(-36 0)">{notchy(0, 0, 72, accent)}</g></g></g></g>'


# --------------------------------------------------------------------------- pictures


def notch():
    """The collapsed island cycling through what it shows; Notchy drips out while Claude works."""
    W, H, cx = 900, 170, 450
    dur, n = 14, 4
    s = 1 / n  # each state's share of the loop
    states = []

    # idle: just the orb and the sessions' colours
    w = 288
    states.append(
        island(cx, w, 32, 14)
        + orb(cx - w / 2 + 20, 16, DIM4, glow=False, cls="breathe")
        + "".join(mini(cx + w / 2 - 22 - i * 16, 16, h, 12) for i, h in enumerate((205, 32, 140)))
    )
    # recording a meeting: the last line said comes in word by word, the timer ticks
    w = 440
    bars = "".join(
        f'<rect x="{cx + w / 2 - 40 + i * 4.5}" y="{16 - 7 * a:.1f}" width="2.5" height="{14 * a:.1f}" rx="1.2" fill="{RED}">'
        f'<animate attributeName="height" values="{14 * a:.1f};14;{14 * a:.1f}" dur="{0.5 + i * 0.13:.2f}s" repeatCount="indefinite"/>'
        f'<animate attributeName="y" values="{16 - 7 * a:.1f};9;{16 - 7 * a:.1f}" dur="{0.5 + i * 0.13:.2f}s" repeatCount="indefinite"/></rect>'
        for i, a in enumerate((0.3, 0.6, 0.4, 0.8, 0.35))
    )
    said = ("so we ship it", "so we ship it on Friday,", "so we ship it on Friday, right?")
    states.append(
        island(cx, w, 32, 14)
        + tone_edge(cx, w, 32, 14, RED, 1)
        + orb(cx - w / 2 + 20, 16, RED)
        + steps_of(dur, [(0, "")] + [(s + i * s / 3, text(cx - w / 2 + 34, 20.5, f"12:0{3 + i}", 12.5, RED_TEXT, 600)) for i in range(3)] + [(2 * s, "")])
        + text(cx - w / 2 + 76, 20.5, "Others:", 12.5, DIM, 600)
        + steps_of(dur, [(0, "")] + [(s + 0.01 + i * 0.05, text(cx - w / 2 + 124, 20.5, line, 12.5, "#e8e9ec")) for i, line in enumerate(said)] + [(2 * s, "")])
        + bars
    )
    # Claude at work: the step it's on changes, Notchy hangs below the island
    a = 2 * s
    work = [("notch-code: Reading notch.css", "00:41"), ("notch-code: Editing App.tsx", "00:42"), ("notch-code: Running npm test", "00:44")]
    states.append(
        mascot_drop(cx, 32, dur, a + 0.02, a + s - 0.01, SKY)
        + island(cx, w, 32, 14)
        + tone_edge(cx, w, 32, 14, SKY, 0.8)
        + orb(cx - w / 2 + 20, 16, SKY, ring=True)
        + steps_of(
            dur,
            [(0, "")]
            + [(a + i * s / 3, shimmer_text(cx - w / 2 + 36, 20.5, label, 12.5, 400) + text(cx + w / 2 - 16, 20.5, t, 11.5, DIM, anchor="end")) for i, (label, t) in enumerate(work)]
            + [(a + s, "")],
        )
    )
    # finished while closed
    states.append(
        island(cx, w, 32, 14)
        + tone_edge(cx, w, 32, 14, GREEN, 1)
        + orb(cx - w / 2 + 20, 16, GREEN)
        + text(cx - w / 2 + 36, 20.5, "notch-code · done in 1 min 12 s", 12.5, INK, 600)
        + text(cx + w / 2 - 16, 20.5, "hover to see", 11, DIM4, anchor="end")
    )

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
    # one label per state; the bar under the current one fills up as its time runs
    labels = ("Nothing going on", "Recording a meeting", "Claude at work", "Done — hover to read")
    ly = H - 32
    for i, lab in enumerate(labels):
        x = 150 + i * 200
        a, b = i * s, (i + 1) * s
        kt, vals = ("0;" + f"{b:.4f}", "1;0") if i == 0 else (f"0;{a:.4f};{b:.4f}", "0;1;0")
        fill = f"0;{a:.4f};{b - 0.0001:.4f};{b:.4f}" + (";1" if b < 1 else "")
        body += f'<rect x="{x - 40}" y="{ly + 10}" width="80" height="2.5" rx="1.25" fill="#fff" fill-opacity="0.08"/>'
        body += (
            f'<rect x="{x - 40}" y="{ly + 10}" width="0" height="2.5" rx="1.25" fill="{INK}">'
            f'<animate attributeName="width" dur="{dur}s" repeatCount="indefinite" keyTimes="{fill}" values="0;0;80;0{";0" if b < 1 else ""}"/></rect>'
        )
        body += (
            f'<text x="{x}" y="{ly}" font-size="12" fill="{DIM3}" font-weight="500" text-anchor="middle">{esc(lab)}'
            f'<animate attributeName="fill" dur="{dur}s" repeatCount="indefinite" keyTimes="{kt}" '
            f'values="{vals.replace("1", INK).replace("0", DIM3)}" calcMode="discrete"/></text>'
        )
    return svg(W, H, body)


def overview():
    """The open island: what's going on with Notchy and what's playing (left), a pill per place to jump into (right)."""
    W, H, cx = 900, 304, 450
    w, h = 640, 258
    L = cx - w / 2
    dur = 9
    body = desktop(W, H) + island(cx, w, h, 22)
    body += header(L, w, [("icon", "home", True, 0), ("icon", "mic", False, 0), ("sep", "", False, 0), ("session", "notch-code", False, 205), ("session", "api", False, 32, "gemini"), ("icon", "plus", False, 0)])

    # status card: the steps tick by while Notchy watches
    x, y, cw, ch = L + 10, 42, 322, 132
    body += card(x, y, cw, ch, (SKY, 0.22))
    body += f'<circle cx="{x + 20}" cy="{y + 21}" r="4" fill="{hsl(205, 85, 68)}"/>'
    body += text(x + 31, y + 25.5, "notch-code", 12.5, INK, 600) + text(x + 105, y + 25.5, "Claude Code", 11.5, DIM2)
    steps = [("read", "Read 4 files"), ("search", "Searched for setNotchSize"), ("edit", "Editing App.tsx"), ("run", "Running npm test")]
    phases = []
    for k in range(3):
        (pi, pl), (_, cl) = steps[k], steps[k + 1]
        ry = y + 42
        rows = '<g opacity="0.7">' + icon(pi, x + 16, ry - 1, 12, DIM2) + text(x + 35, ry + 9.5, pl, 13, DIM3, 500) + "</g>"
        rows += spinner(x + 22, ry + 27, SKY) + shimmer_text(x + 35, ry + 31.5, cl)
        rows += text(x + cw - 16, y + 25.5, f"00:{40 + k * 3}", 11, DIM3, anchor="end")
        phases.append((k / 3, rows))
    body += steps_of(dur, phases)
    body += notchy(x + cw - 14 - 56, y + ch - 48 - 51, 56, SKY, tile=True)
    b1, bw = btn(x + 16, y + ch - 36, "Watch", True)
    b2, _ = btn(x + 16 + bw + 8, y + ch - 36, "Stop", icon="stop")
    body += b1 + b2

    # now playing, under the status card
    my = y + ch + 8
    body += card(x, my, cw, 66, r=18)
    g = uid("cover")
    _defs.append(
        f'<linearGradient id="{g}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#f472b6"/><stop offset="0.5" stop-color="#8b5cf6"/><stop offset="1" stop-color="#1e1b4b"/></linearGradient>'
    )
    c = uid("clip")
    _defs.append(f'<clipPath id="{c}"><rect x="{x + 8}" y="{my + 9}" width="48" height="48" rx="10"/></clipPath>')
    body += (
        f'<g clip-path="url(#{c})"><rect x="{x + 8}" y="{my + 9}" width="48" height="48" fill="url(#{g})"/>'
        f'<circle cx="{x + 40}" cy="{my + 25}" r="7" fill="#fde68a" opacity="0.85"/>'
        f'<path d="M{x + 8} {my + 47} q12 -10 24 -2 t24 -4 V{my + 57} H{x + 8} Z" fill="#0f0a2a" opacity="0.7"/></g>'
    )
    body += text(x + 66, my + 25, "Midnight City", 12.5, INK, 600)
    body += text(x + 66, my + 40, "M83", 11, DIM) + text(x + 91, my + 40, "· Spotify", 11, DIM4)
    bx, bw2 = x + 66, 122
    body += f'<rect x="{bx}" y="{my + 49}" width="{bw2}" height="3" rx="1.5" fill="#fff" fill-opacity="0.08"/>'
    body += (
        f'<rect x="{bx}" y="{my + 49}" width="{bw2 * 0.42:.0f}" height="3" rx="1.5" fill="{GREEN2}">'
        f'<animate attributeName="width" dur="{dur}s" repeatCount="indefinite" values="{bw2 * 0.42:.0f};{bw2 * 0.5:.0f}"/></rect>'
    )
    body += steps_of(dur, [(k / 3, text(bx + bw2 + 6, my + 53, f"1:{42 + k * 3}", 10, DIM4, extra='class="mono"')) for k in range(3)])
    px0 = x + cw - 10 - 86
    body += icon("prev", px0 + 6.5, my + 26.5, 13, DIM)
    body += f'<circle cx="{px0 + 43}" cy="{my + 33}" r="15" fill="{INK}"/>' + icon("pause", px0 + 36.5, my + 26.5, 13, "#0b0c0e")
    body += icon("next", px0 + 66.5, my + 26.5, 13, DIM)

    # pills card: notch-code works, api (Gemini) finishes partway through
    px, pw, ph = x + cw + 10, w - cw - 30, h - 52
    body += card(px, y, pw, ph)
    pills = [
        ("rec", "Transcribe", None, False, None),
        ("session", "notch-code", 205, True, "claude"),
        ("session", "api", 32, False, "gemini"),
        ("session", "website", 140, False, "claude"),
        ("ghost", "dotfiles", 280, False, None),
        ("ghost", "blog", 330, False, None),
        ("plus", "New session", None, False, None),
    ]
    colw = (pw - 26) / 2
    rows = (len(pills) + 1) // 2
    top = y + (ph - (rows * 32 - 4)) / 2
    for i, (kind, label, hue, busy, provider) in enumerate(pills):
        qx = px + 10 + (i % 2) * (colw + 6)
        qy = top + (i // 2) * 32
        mid = qx + colw / 2 + 8
        if kind == "session":
            stroke = hsl(hue, 75, 62, 0.85) if busy else hsl(hue, 55, 45, 0.55)
            tw = len(label) * 6.3
            body += f'<rect x="{qx}" y="{qy}" width="{colw}" height="28" rx="14" fill="{hsl(hue, 40, 11)}" stroke="{stroke}"/>'
            body += mini(qx + 8, qy + 14, hue, 14, busy) + text(mid - 9, qy + 18, label, 11, hsl(hue, 80, 76), 600, "middle")
            body += agent_tag(mid - 9 + tw / 2 + 5, qy + 14, provider)
            if label == "api":  # its reply lands: the "new reply" dot
                dot = f'<circle cx="{qx + colw - 13}" cy="{qy + 14}" r="6" fill="{GREEN}" opacity="0.25"/><circle cx="{qx + colw - 13}" cy="{qy + 14}" r="3.5" fill="{GREEN}"/>'
                body += during(dur, 0.55, 0.97, dot, 0.03)
        elif kind == "ghost":
            body += f'<rect x="{qx}" y="{qy}" width="{colw}" height="28" rx="14" fill="none" stroke="#fff" stroke-opacity="0.12" stroke-dasharray="3 3"/>'
            body += mini(qx + 8, qy + 14, hue, 14) + text(mid, qy + 18, label, 11, DIM3, 600, "middle")
        elif kind == "plus":
            body += f'<rect x="{qx}" y="{qy}" width="{colw}" height="28" rx="14" fill="none" stroke="#fff" stroke-opacity="0.12" stroke-dasharray="3 3"/>'
            body += f'<circle cx="{qx + 15}" cy="{qy + 14}" r="9" fill="#fff" fill-opacity="0.07"/>' + icon("plus", qx + 9.5, qy + 8.5, 11, DIM2)
            body += text(mid, qy + 18, label, 11, DIM3, 600, "middle")
        else:
            body += f'<rect x="{qx}" y="{qy}" width="{colw}" height="28" rx="14" fill="#0e0f11" stroke="#fff" stroke-opacity="0.14"/>'
            body += f'<circle cx="{qx + 15}" cy="{qy + 14}" r="9" fill="{RED}" fill-opacity="0.16"/>' + icon("mic", qx + 9.5, qy + 8.5, 11, RED_TEXT)
            body += text(mid, qy + 18, label, 11, DIM, 600, "middle")
    return svg(W, H, body)


def session():
    """A Claude session in Auto mode: steps land one by one, tokens count up, a follow-up gets typed."""
    W, H, cx = 900, 476, 450
    w, h = 640, 426
    L = cx - w / 2
    dur = 10
    body = desktop(W, H) + island(cx, w, h, 22)
    body += header(L, w, [("icon", "home", False, 0), ("icon", "mic", False, 0), ("sep", "", False, 0), ("session", "notch-code", True, 205), ("session", "api", False, 32, "gemini"), ("icon", "plus", False, 0)])
    x, y, cw, ch = L + 10, 42, w - 20, h - 52
    body += card(x, y, cw, ch, (AMBER, 0.26))

    # card head
    body += f'<rect x="{x + 14}" y="{y + 13}" width="118" height="26" rx="13" fill="#fff" fill-opacity="0.07"/>'
    body += mini(x + 24, y + 26, 205, 10, True) + text(x + 41, y + 30.5, "notch-code", 12.5, INK, 600) + icon("chevron", x + 112, y + 21, 11, DIM2)
    body += text(x + 142, y + 30.5, "Claude · working", 11, DIM3)
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

    # steps land one by one: a spinner while it's the current one, a check once the next starts
    steps = [("read", "Read 6 files", DIM3, "00:03"), ("edit", "Editing notch.css", CYAN, "00:09"), ("edit", "Editing App.tsx", CYAN, "00:07"), ("run", "Running npm run build", AMBER, "00:12")]
    starts = [0, 0.2, 0.4, 0.6]
    tokens = ["0.6k", "1.3k", "2.1k", "2.4k"]
    for i, (ic, label, col, t) in enumerate(steps):
        t0, t1 = starts[i], starts[i + 1] if i + 1 < len(steps) else 0.97
        head = icon(ic, x + 18, fy - 9, 12, col)
        running = head + text(x + 37, fy + 0.5, label, 12.5, INK) + text(x + cw - 50, fy + 0.5, t, 10.5, "#9cd8fb", anchor="end") + spinner(x + cw - 28, fy - 3.5, SKY)
        done = head + text(x + 37, fy + 0.5, label, 12.5, "#b0b5be") + icon("check", x + cw - 34, fy - 9, 12, GREEN)
        body += during(dur, t0, t1, running, 0.02 if t0 else 0)
        if i + 1 < len(steps):
            body += during(dur, t1, 0.97, done)
        fy += 24

    # run strip (Auto): the current step, and a token count that climbs
    sy = y + ch - 120
    body += f'<rect x="{x + 14}" y="{sy}" width="{cw - 28}" height="30" rx="12" fill="{AMBER}" fill-opacity="0.07" stroke="{AMBER}" stroke-opacity="0.2"/>'
    body += spinner(x + 30, sy + 15, AMBER)
    body += f'<rect x="{x + 42}" y="{sy + 8}" width="40" height="15" rx="7.5" fill="{AMBER}" fill-opacity="0.16"/>' + text(x + 62, sy + 19, "AUTO", 9.5, AMBER_TEXT, 700, "middle")
    body += steps_of(
        dur,
        [
            (
                starts[i],
                shimmer_text(x + 92, sy + 19.5, label, 12, 500)
                + text(x + cw - 150, sy + 19.5, f"↓ {tokens[i]} tokens", 11, DIM2, anchor="end")
                + text(x + cw - 100, sy + 19.5, f"00:{46 + i * 4}", 11, DIM2, anchor="end"),
            )
            for i, (_, label, _, _) in enumerate(steps)
        ],
    )
    sb, _ = btn(x + cw - 92, sy + 4, "", size=11, w=72)
    body += sb + text(x + cw - 80, sy + 19.5, "Stop", 11.5, "#f1f2f4", 500)
    body += f'<rect x="{x + cw - 50}" y="{sy + 9}" width="24" height="15" rx="4" fill="#fff" fill-opacity="0.1"/>' + text(x + cw - 38, sy + 19.5, "Esc", 9.5, DIM, 600, "middle")

    # chat bar: a follow-up gets typed while Claude works
    cy = sy + 38
    body += f'<rect x="{x + 14}" y="{cy}" width="{cw - 28}" height="42" rx="14" fill="#fff" fill-opacity="0.07"/>'
    body += during(dur, 0, 0.62, text(x + 28, cy + 25.5, "Add more details — Claude reads it at its next step", 13, DIM3))
    body += typed(x + 28, cy + 25.5, "and keep the pills the same height", dur, 0.62, 0.86)
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
    """Live transcription: each line shows up as a grey live guess, then settles; the timer ticks."""
    W, H, cx = 900, 340, 450
    w, h = 640, 296
    L = cx - w / 2
    dur = 9
    body = desktop(W, H) + island(cx, w, h, 22)
    body += header(L, w, [("icon", "home", False, 0), ("rec", "", True, 0), ("sep", "", False, 0), ("session", "notch-code", False, 205), ("icon", "plus", False, 0)])
    x, y, cw, ch = L + 10, 42, w - 20, h - 52
    body += card(x, y, cw, ch, (RED, 0.24))
    clock = [k / 4 for k in range(4)]
    body += steps_of(dur, [(t, text(L + 71, 30, f"12:0{1 + k}", 11.5, RED_TEXT, 600)) for k, t in enumerate(clock)])

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
    body += steps_of(dur, [(t, text(x + cw - 196, hy + 20, f"12:0{1 + k}", 15, RED_TEXT, 600, "end")) for k, t in enumerate(clock)])
    body += icon("folder", x + cw - 186, hy + 8, 14, DIM2)
    p1, pw = btn(x + cw - 160, hy + 3, "Pause")
    p2, _ = btn(x + cw - 160 + pw + 8, hy + 3, "End", True, icon="stop")
    body += p1 + p2
    body += f'<rect x="{x + 14}" y="{hy + 44}" width="{cw - 28}" height="1" fill="#fff" fill-opacity="0.06"/>'

    # transcript: each new line arrives as an italic grey guess, then the final text replaces it
    lines = [
        ("11:41", "others", "Okay, the release branch is cut."),
        ("11:46", "me", "Did the Vulkan build pass on the AMD box?"),
        ("11:52", "others", "Yes, both machines, no regressions."),
        ("11:57", "me", "Great — then the notch redesign goes in too."),
        ("12:01", "others", "So we ship it on Friday, right?"),
        ("12:04", "me", "Friday it is."),
    ]
    ty = hy + 68
    for i, (ts, who, s) in enumerate(lines):
        color = "#67e8f9" if who == "me" else "#fcd34d"
        lead = text(x + 16, ty, ts, 11, DIM4) + text(x + 62, ty, "Me" if who == "me" else "Others", 12, color, 600)
        row = lead + text(x + 124, ty, s, 12.5, TEXT)
        if i < 2:  # already there when the picture is still
            body += row
        else:
            t = 0.06 + (i - 2) * 0.2
            words = s.split()
            partial = lead + text(x + 124, ty, " ".join(words[: max(2, len(words) // 2)]) + "…", 12.5, DIM, extra='font-style="italic"')
            body += during(dur, t, t + 0.08, partial, 0.02) + during(dur, t + 0.08, 0.96, row, 0.015)
        ty += 24
    return svg(W, H, body)


def main():
    OUT.mkdir(exist_ok=True)
    for name, fn in (("notch", notch), ("overview", overview), ("session", session), ("transcribe", transcribe)):
        (OUT / f"{name}.svg").write_text(fn(), encoding="utf-8")
        print(f"docs/{name}.svg")


if __name__ == "__main__":
    main()
