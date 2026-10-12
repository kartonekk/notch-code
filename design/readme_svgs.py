"""Animated mockups for the README (docs/*.svg).

GitHub shows SVGs in <img> tags without scripts, so everything moves with SMIL
(<animate>, timings in one loop) and CSS keyframes (Hum's own breathing, blinking,
sound bars). Hum's markup and styles mirror app/src/Hum.tsx and the .hum rules in
app/src/notch.css.

    python design/readme_svgs.py
"""

import math
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs"

# --------------------------------------------------------------------------- styles

HUM_CSS = re.search(
    r"/\* -{10,} Hum \*/(.*?)@media \(prefers-reduced-motion",
    (ROOT / "app/src/notch.css").read_text(encoding="utf-8"),
    re.S,
).group(1)

BASE_CSS = """
text { font-family: "Segoe UI Variable Text", "Segoe UI", -apple-system, BlinkMacSystemFont, "Helvetica Neue", Arial, sans-serif; }
.hum { --ease: cubic-bezier(0.32, 0.72, 0, 1); --lvl: 0.6; }
/* README-only: moods that play once in the app loop here */
.loop-hop .hum-body { animation: hum-hop 1.7s var(--ease) infinite !important; }
.loop-shake .hum-body { animation: hum-shake-loop 2.2s ease-in-out infinite !important; }
@keyframes hum-shake-loop { 0%, 40%, 100% { transform: none; } 10%, 30% { transform: translateX(-2px); } 20% { transform: translateX(2px); } }
@media (prefers-reduced-motion: reduce) { * { animation: none !important; } }
"""


def svg(w, h, body, extra_css=""):
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<style>{HUM_CSS}{BASE_CSS}{extra_css}</style>
{body}
</svg>
"""


# --------------------------------------------------------------------------- Hum


def hum(x, y, w, mood, hue=None, cls="", face_anim=""):
    """Hum at (x, y), `w` wide (Hum is 64:44)."""
    style = f' style="--body: hsl({hue} 75% 76%); --blink-delay: {-(hue % 50) / 10}s"' if hue is not None else ""
    return f"""<g transform="translate({x} {y}) scale({w / 64:.4f})">
<g class="hum hum-{mood} {cls}"{style}>
  <g class="hum-body">
    <path class="hum-band" d="M7.75 21 C7.75 3.5 56.25 3.5 56.25 21"/>
    <rect class="hum-shape" x="9" y="13" width="46" height="29" rx="14.5"/>
    <ellipse class="hum-shine" cx="19" cy="18" rx="5" ry="2.2" transform="rotate(-16 19 18)"/>
    <rect class="hum-shell" x="3" y="20" width="9.5" height="16" rx="4.75"/>
    <rect class="hum-shell" x="51.5" y="20" width="9.5" height="16" rx="4.75"/>
    <rect class="hum-cup" x="4.6" y="22" width="6.3" height="12" rx="3.15"/>
    <rect class="hum-cup" x="53.1" y="22" width="6.3" height="12" rx="3.15"/>
    <g class="hum-eq"><rect x="5.5" y="25" width="1.2" height="6" rx="0.6"/><rect x="7.15" y="25" width="1.2" height="6" rx="0.6"/><rect x="8.8" y="25" width="1.2" height="6" rx="0.6"/><rect x="54" y="25" width="1.2" height="6" rx="0.6"/><rect x="55.65" y="25" width="1.2" height="6" rx="0.6"/><rect x="57.3" y="25" width="1.2" height="6" rx="0.6"/></g>
    <g class="hum-face"><g>{face_anim}
      <g class="hum-eyes">
        <rect class="hum-eye" x="23" y="24" width="4.4" height="7" rx="2.2"/>
        <rect class="hum-eye" x="36.6" y="24" width="4.4" height="7" rx="2.2"/>
      </g>
      <path class="hum-line hum-smile" d="M22.8 28.5 q2.4 -3.6 4.8 0 M36.4 28.5 q2.4 -3.6 4.8 0"/>
      <path class="hum-line hum-shut" d="M23 27.5 h4.4 M36.6 27.5 h4.4"/>
      <ellipse class="hum-mouth" cx="32" cy="35" rx="1.9" ry="1.5"/>
      <ellipse class="hum-blush" cx="18" cy="32.5" rx="2.4" ry="1.4"/>
      <ellipse class="hum-blush" cx="46" cy="32.5" rx="2.4" ry="1.4"/>
      <g class="hum-dots"><circle cx="28" cy="36.5" r="1.1"/><circle cx="32" cy="36.5" r="1.1"/><circle cx="36" cy="36.5" r="1.1"/></g>
    </g></g>
  </g>
</g>
</g>"""


# --------------------------------------------------------------------------- SMIL helpers


def keyed(attr, pts, dur, discrete=False, tag="animate", extra=""):
    pts = sorted(pts, key=lambda p: p[0])
    clean = []
    for t, v in pts:
        t = min(max(t, 0), dur)
        if clean and t <= clean[-1][0]:
            t = clean[-1][0] + 0.001
        clean.append((t, v))
    if clean[0][0] > 0:
        clean.insert(0, (0, clean[0][1]))
    if clean[-1][0] < dur:
        clean.append((dur, clean[-1][1]))
    clean[-1] = (dur, clean[-1][1])
    kt = ";".join(f"{t / dur:.4f}" for t, _ in clean)
    vals = ";".join(str(v) for _, v in clean)
    mode = ' calcMode="discrete"' if discrete else ""
    return f'<{tag} attributeName="{attr}" dur="{dur}s" repeatCount="indefinite" keyTimes="{kt}" values="{vals}"{mode}{extra}/>'


def shown(windows, dur, fade=0.3):
    """opacity animation: visible during each (start, end) window"""
    pts = []
    for s, e in windows:
        pts += [(0, 1)] if s <= 0 else [(s - fade, 0), (s, 1)]
        pts += [(dur, 1)] if e >= dur else [(e, 1), (e + fade, 0)]
    if pts[0][0] > 0:
        pts.insert(0, (0, 0))
    if pts[-1][0] < dur:
        pts.append((dur, 0))
    return keyed("opacity", pts, dur)


def scene(windows, dur, content, fade=0.3):
    first = 1 if windows[0][0] <= 0 else 0
    return f'<g opacity="{first}">{shown(windows, dur, fade)}{content}</g>'


def notch_d(cx, w, h, r, ear=12):
    l, rr = cx - w / 2, cx + w / 2
    return (
        f"M{l - ear} 0 Q{l} 0 {l} {ear} V{h - r} Q{l} {h} {l + r} {h} "
        f"H{rr - r} Q{rr} {h} {rr} {h - r} V{ear} Q{rr} 0 {rr + ear} 0 Z"
    )


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def text(x, y, s, size=13, fill="#e5e5ea", weight=400, anchor="start", extra=""):
    return f'<text x="{x}" y="{y}" font-size="{size}" fill="{fill}" font-weight="{weight}" text-anchor="{anchor}"{extra}>{esc(s)}</text>'


WALLPAPER = """<defs>
  <linearGradient id="wall" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#2b3a5c"/><stop offset=".55" stop-color="#3a2f55"/><stop offset="1" stop-color="#1c2233"/>
  </linearGradient>
  <radialGradient id="peach" cx=".5" cy="1.15" r=".7"><stop offset="0" stop-color="#ffaa78" stop-opacity=".22"/><stop offset="1" stop-color="#ffaa78" stop-opacity="0"/></radialGradient>
  <radialGradient id="red" cx=".5" cy="1.15" r=".7"><stop offset="0" stop-color="#ff453a" stop-opacity=".22"/><stop offset="1" stop-color="#ff453a" stop-opacity="0"/></radialGradient>
  <filter id="soft" x="-20%" y="-50%" width="140%" height="200%"><feGaussianBlur stdDeviation="8"/></filter>
</defs>"""


def desktop(w, h):
    """wallpaper with a faint editor window, so the notch has something to sit on"""
    lines = "".join(
        f'<rect x="{150 + (i % 3) * 18}" y="{h - 210 + i * 16}" width="{180 + (i * 97) % 260}" height="6" rx="3" fill="#fff" opacity=".08"/>'
        for i in range(9)
    )
    return f"""{WALLPAPER}
<rect width="{w}" height="{h}" rx="18" fill="url(#wall)"/>
<rect x="110" y="{h - 240}" width="{w - 220}" height="230" rx="12" fill="#0d1018" opacity=".55"/>
<circle cx="130" cy="{h - 226}" r="4" fill="#ff5f57" opacity=".6"/><circle cx="144" cy="{h - 226}" r="4" fill="#febc2e" opacity=".6"/><circle cx="158" cy="{h - 226}" r="4" fill="#28c840" opacity=".6"/>
{lines}"""


# --------------------------------------------------------------------------- 1. the collapsed notch


def hero():
    W, H, CX, D = 900, 300, 450, 15.0
    small, wide = 280, 460
    A, B, C, E = (0, 3), (3.4, 8.2), (8.6, 11.8), (12.2, 14.6)
    widths = [(0, small), (3.0, small), (3.4, wide), (14.6, wide), (15.0, small)]
    d_pts = [(t, notch_d(CX, w, 36, 18)) for t, w in widths]
    lw, lsm = CX - wide / 2, CX - small / 2

    timers = "".join(
        scene([(B[0] + i, B[0] + i + 1 if i < 4 else B[1])], D, text(lw + 60, 23, f"12:{41 + i:02d}", 13, "#ff6961", 600, extra=' font-variant-numeric="tabular-nums"'), 0.01)
        for i in range(5)
    )
    rec = scene([B], D, f"""
      {hum(lw + 8, 4.5, 40, "listen", cls="")}
      {timers}
      {scene([(B[0], 5.8)], D, f'<text x="{lw + 106}" y="23" font-size="13" fill="#e5e5ea"><tspan font-weight="700">Others:</tspan> so the release goes out on Thursday?</text>')}
      {scene([(6.0, B[1])], D, f'<text x="{lw + 106}" y="23" font-size="13" fill="#e5e5ea"><tspan font-weight="700">Me:</tspan> yes, right after the review is merged.</text>')}
    """)
    work = scene([C], D, f"""
      {hum(lw + 8, 4.5, 40, "work")}
      {scene([(C[0], 10.2)], D, text(lw + 60, 23, "notch-code: Editing App.tsx"))}
      {scene([(10.4, C[1])], D, text(lw + 60, 23, "notch-code: Running npm test"))}
      {text(CX + wide / 2 - 16, 23, "01:12", 12, "#8e8e93", anchor="end")}
    """)
    done = scene([E], D, f"""
      {hum(lw + 8, 4.5, 40, "happy", cls="loop-hop")}
      {text(lw + 60, 23, "notch-code finished — 14 steps")}
      {text(CX + wide / 2 - 16, 23, "hover to see", 12, "#8e8e93", anchor="end")}
    """)
    idle = scene([A, (14.9, D)], D, hum(lsm + 8, 4.5, 40, "sleep"))

    labels = ["Idle — Hum naps", "Recording a meeting", "Claude working", "Done"]
    wins = [[A, (14.9, D)], [B], [C], [E]]
    pills = ""
    x = CX - 330
    for lab, win in zip(labels, wins):
        pw = len(lab) * 7 + 28
        pills += f"""<g transform="translate({x} {H - 44})">
          <rect width="{pw}" height="28" rx="14" fill="#000" opacity=".35"/>
          <g opacity="0">{shown(win, D)}<rect width="{pw}" height="28" rx="14" fill="#fff" opacity=".16"/></g>
          {text(pw / 2, 18.5, lab, 12.5, "#f5f5f7", 600, "middle")}
        </g>"""
        x += pw + 10

    glow = f'<path d="{notch_d(CX, wide, 36, 18)}" fill="#30d158" filter="url(#soft)" opacity="0">{shown([E], D)}</path>'
    body = f"""{desktop(W, H)}
{glow}
<path d="{notch_d(CX, small, 36, 18)}" fill="#000">{keyed("d", d_pts, D)}</path>
{idle}{rec}{work}{done}
{pills}"""
    return svg(W, H, body)


# --------------------------------------------------------------------------- shared panel pieces


def card(x, y, w, h, glow=None):
    g = f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="20" fill="url(#{glow})"/>' if glow else ""
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="20" fill="#111214" stroke="#fff" stroke-opacity=".07"/>{g}'


def pill(x, y, w, h, fill, label="", color="#f5f5f7", size=12.5, weight=600, stroke=None):
    s = f' stroke="{stroke}"' if stroke else ""
    t = text(x + w / 2, y + h / 2 + size * 0.36, label, size, color, weight, "middle") if label else ""
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{h / 2}" fill="{fill}"{s}/>{t}'


def icon_mic(x, y, color="#8e8e93"):
    return f'<g transform="translate({x} {y}) scale(.55)" fill="none" stroke="{color}" stroke-width="2.2" stroke-linecap="round"><rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10a7 7 0 0 0 14 0M12 17v5"/></g>'


def icon_pin(x, y):
    return f'<g transform="translate({x} {y}) scale(.6)" fill="none" stroke="#8e8e93" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 17v5M9 3h6l-1 7 4 3v2H6v-2l4-3-1-7z"/></g>'


def icon_power(x, y):
    return f'<g transform="translate({x} {y}) scale(.6)" fill="none" stroke="#8e8e93" stroke-width="2" stroke-linecap="round"><path d="M12 3v9M6.3 6.3a8 8 0 1 0 11.4 0"/></g>'


SESSIONS = [("notch-code", 214, "work"), ("api-server", 142, "happy")]


def topbar(L, active, rec_timer=None):
    """tabs: Hum (overview), Transcribe, sessions, + ; chips and icons on the right"""
    y = 10
    out = ""
    if active == "home":
        out = f'<rect x="{L + 14}" y="{y}" width="44" height="30" rx="15" fill="#fff" fill-opacity=".12"/>'
    out += hum(L + 21, y + 4.5, 30, "idle")
    # transcribe tab
    tx = L + 64
    if active == "transcribe":
        out += f'<rect x="{tx}" y="{y}" width="84" height="30" rx="15" fill="#fff" fill-opacity=".12"/>'
        out += f'<circle cx="{tx + 16}" cy="{y + 15}" r="4" fill="#ff6961"><animate attributeName="opacity" values="1;.4;1" dur="1.4s" repeatCount="indefinite"/></circle>'
        out += rec_timer or ""
    else:
        out += icon_mic(tx + 9, y + 8) + text(tx + 28, y + 19.5, "Transcribe", 12.5, "#8e8e93", 600)
    out += f'<rect x="{tx + 106}" y="{y + 7}" width="1" height="16" fill="#fff" opacity=".1"/>'
    sx = tx + 114
    for name, h, mood in SESSIONS:
        tw = 30 + len(name) * 7.2
        out += hum(sx + 7, y + 7.5, 24, mood, hue=h, cls="loop-hop" if mood == "happy" else "")
        out += text(sx + 36, y + 19.5, name, 12.5, "#8e8e93", 600)
        sx += tw + 8
    out += text(sx + 12, y + 20, "+", 17, "#8e8e93", 500, "middle")
    # right side
    R = L + 680 - 14
    out += f'<rect x="{R - 166}" y="{y + 4}" width="72" height="22" rx="11" fill="#fff" fill-opacity=".06"/>'
    out += text(R - 156, y + 19, "5h", 11, "#8e8e93", 600)
    out += f'<rect x="{R - 138}" y="{y + 13}" width="26" height="4" rx="2" fill="#fff" fill-opacity=".1"/><rect x="{R - 138}" y="{y + 13}" width="16" height="4" rx="2" fill="#30d158"/>'
    out += text(R - 108, y + 19, "62%", 11, "#8e8e93", 600)
    out += icon_mic(R - 82, y + 8, "#0a84ff") + icon_pin(R - 54, y + 8) + icon_power(R - 26, y + 8)
    return out


# --------------------------------------------------------------------------- 2. the overview


def panel():
    W, H, CX, D = 900, 560, 450, 13.0
    PW, PH = 680, 500
    L = CX - PW / 2
    d_pts = [
        (0, notch_d(CX, 280, 36, 18)),
        (1.0, notch_d(CX, 280, 36, 18)),
        (1.45, notch_d(CX, PW, PH, 28)),
        (11.2, notch_d(CX, PW, PH, 28)),
        (11.65, notch_d(CX, 280, 36, 18)),
    ]
    OPEN = (1.65, 10.95)

    # cursor path; the big Hum's eyes follow it
    hero_c = (L + 14 + 125, 50 + 210)
    path = [(0, (840, 560)), (0.9, (470, 22)), (2.4, (300, 330)), (3.8, (520, 100)), (5.2, (700, 100)),
            (6.6, (520, 168)), (8.0, (700, 168)), (9.4, (330, 380)), (11.0, (470, 26)), (12.2, (840, 560)), (D, (840, 560))]

    def look(p):
        dx, dy = p[0] - hero_c[0], p[1] - hero_c[1]
        d = math.hypot(dx, dy) or 1
        k = min(1, d / 200)
        return f"{dx / d * k * 2.6:.2f} {dy / d * k * 1.8:.2f}"

    face = keyed("transform", [(t, look(p)) for t, p in path], D, tag="animateTransform", extra=' type="translate"')
    cursor = f"""<g>{keyed("transform", [(t, f"{p[0]} {p[1]}") for t, p in path], D, tag="animateTransform", extra=' type="translate"')}
      <path d="M0 0 V17 L4.5 13 L7.6 20 L10.6 18.7 L7.6 12 H13.4 Z" fill="#fff" stroke="#000" stroke-width="1.2" stroke-linejoin="round"/></g>"""

    # hero card
    hx, hw, cy, ch = L + 14, 250, 50, 436
    mid = hx + hw / 2
    hero_card = f"""{card(hx, cy, hw, ch, "peach")}
      {hum(mid - 75, 158, 150, "idle", face_anim=face)}
      {text(mid, 296, "Hi! What's next?", 16, "#f5f5f7", 700, "middle")}
      {text(mid, 320, "Record a meeting, or ask", 12.5, "#8e8e93", 400, "middle")}
      {text(mid, 337, "Claude to do something.", 12.5, "#8e8e93", 400, "middle")}
      <rect x="{mid - 92}" y="356" width="98" height="32" rx="10" fill="#ff453a"/><circle cx="{mid - 75}" cy="372" r="4.5" fill="#fff"/>
      {text(mid - 66, 376.5, "Transcribe", 12.5, "#fff", 600)}
      {text(mid + 50, 376.5, "Ask Claude", 12.5, "#d1d1d6", 600, "middle")}"""

    # tiles
    tx0, tw = L + 274, PW - 288
    tiles_card = card(tx0, cy, tw, ch)
    col = [tx0 + 12, tx0 + 12 + (tw - 32) / 2 + 8]
    cw = (tw - 32) / 2
    rows = [62, 126, 190]
    tiles = [
        ("Transcribe", "Record any app", hum(0, 0, 44, "idle"), None, False, (3.4, 4.6)),
        ("notch-code", "Editing App.tsx · 00:42", hum(0, 0, 44, "work", hue=214), "#5aa0ff", False, (4.8, 6.0)),
        ("api-server", "Done — new reply", hum(0, 0, 44, "happy", hue=142, cls="loop-hop"), "#30d158", False, (6.2, 7.4)),
        ("website", "Open a session here", hum(0, 0, 44, "sleep", hue=28), None, True, (7.6, 8.8)),
        ("New Claude session", "Pick a folder", None, None, True, None),
    ]
    tile_svg = ""
    for i, (title, sub, h, border, ghost, hover) in enumerate(tiles):
        x, y = col[i % 2], cy + 12 + (i // 2) * 64
        stroke = border or "#fff"
        sop = ".5" if border else (".12" if ghost else ".06")
        dash = ' stroke-dasharray="4 3"' if ghost else ""
        fill = "none" if ghost else "#fff"
        tile_svg += f'<rect x="{x}" y="{y}" width="{cw}" height="56" rx="14" fill="{fill}" fill-opacity=".04" stroke="{stroke}" stroke-opacity="{sop}"{dash}/>'
        if hover:
            tile_svg += f'<rect x="{x}" y="{y}" width="{cw}" height="56" rx="14" fill="#fff" fill-opacity=".06" opacity="0">{shown([hover], D, .15)}</rect>'
        if h:
            tile_svg += f'<g transform="translate({x + 6} {y + 13})">{h}</g>'
        else:
            tile_svg += f'<rect x="{x + 12}" y="{y + 12}" width="32" height="32" rx="11" fill="#fff" fill-opacity=".06"/>' + text(x + 28, y + 34, "+", 18, "#8e8e93", 500, "middle")
        tile_svg += text(x + 58, y + 25, title, 13, "#f5f5f7", 700) + text(x + 58, y + 42, sub, 11.5, "#8e8e93")

    content = scene([OPEN], D, f"""{topbar(L, "home")}{hero_card}{tiles_card}{tile_svg}""", 0.25)
    bar = scene([(0, 1.05), (11.6, D)], D, hum(CX - 140 + 8, 4.5, 40, "sleep"), 0.15)
    body = f"""{desktop(W, H)}
<path d="{notch_d(CX, 280, 36, 18)}" fill="#000">{keyed("d", d_pts, D)}</path>
{bar}{content}{cursor}"""
    return svg(W, H, body)


# --------------------------------------------------------------------------- 3. live transcription


def transcribe():
    W, H, CX, D = 900, 560, 450, 14.0
    PW, PH = 680, 500
    L = CX - PW / 2
    cy, ch = 50, 436
    cx0, cw = L + 14, PW - 28

    timer_frames = "".join(
        scene([(i, i + 1)], D, text(L + 102, 29.5, f"12:{31 + i:02d}", 12.5, "#ff6961", 600), 0.01) for i in range(int(D))
    )
    big_timer = "".join(
        scene([(i, i + 1)], D, text(cx0 + cw - 200, 92, f"12:{31 + i:02d}", 22, "#f5f5f7", 600, "end"), 0.01) for i in range(int(D))
    )

    def meter(x, y, seq, dur):
        vals = ";".join(str(v) for v in seq)
        return f'<rect x="{x}" y="{y}" width="60" height="5" rx="2.5" fill="#fff" fill-opacity=".08"/><rect x="{x}" y="{y}" height="5" rx="2.5" fill="#30d158" width="20"><animate attributeName="width" values="{vals}" dur="{dur}s" repeatCount="indefinite"/></rect>'

    head = f"""{hum(cx0 + 14, 66, 52, "listen", cls="")}
      <text x="{cx0 + 80}" y="83" font-size="13.5" fill="#f5f5f7" font-weight="700">Recording <tspan fill="#8e8e93" font-weight="400">· Microsoft Teams</tspan></text>
      {text(cx0 + 80, 105, "App", 11, "#8e8e93")}{meter(cx0 + 104, 100, [12, 44, 30, 58, 22, 50, 16, 40, 12], 1.9)}
      {text(cx0 + 174, 105, "Mic", 11, "#8e8e93")}{meter(cx0 + 198, 100, [4, 6, 30, 48, 10, 4, 36, 8, 4], 2.3)}
      {big_timer}
      <g transform="translate({cx0 + cw - 186} 76)" fill="none" stroke="#8e8e93" stroke-width="1.6"><path d="M2 5a1.5 1.5 0 0 1 1.5-1.5h3l1.5 1.5h6a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5h-10.5a1.5 1.5 0 0 1-1.5-1.5z"/></g>
      <rect x="{cx0 + cw - 158}" y="72" width="76" height="32" rx="10" fill="#fff" fill-opacity=".1"/>
      <rect x="{cx0 + cw - 146}" y="83" width="2.6" height="10" rx="1" fill="#f5f5f7"/><rect x="{cx0 + cw - 140}" y="83" width="2.6" height="10" rx="1" fill="#f5f5f7"/>
      {text(cx0 + cw - 130, 92.5, "Pause", 12.5, "#f5f5f7", 600)}
      <rect x="{cx0 + cw - 74}" y="72" width="62" height="32" rx="10" fill="#ff453a" fill-opacity=".18"/>
      <rect x="{cx0 + cw - 62}" y="83" width="10" height="10" rx="2" fill="#ff6961"/>
      {text(cx0 + cw - 46, 92.5, "End", 12.5, "#ff6961", 600)}
      <rect x="{cx0 + 14}" y="120" width="{cw - 28}" height="1" fill="#fff" fill-opacity=".09"/>"""

    lines = [
        ("12:31", "Others", "Okay, let's go through the release plan for Thursday."),
        ("12:33", "Me", "The build is green — I'm just waiting for one review."),
        ("12:36", "Others", "Great. Can the transcripts go straight into the wiki?"),
        ("12:38", "Me", "Yes, they're plain Markdown files in Documents\\Notch Code."),
        ("12:40", "Others", "And none of this audio leaves the laptop, right?"),
    ]
    body_lines = ""
    for i, (ts, who, said) in enumerate(lines):
        y = 150 + i * 32
        t0 = 0.6 + i * 1.9
        color = "#64d2ff" if who == "Me" else "#ffd60a"
        row = (
            text(cx0 + 18, y, ts, 11.5, "#636366", extra=' font-variant-numeric="tabular-nums"')
            + text(cx0 + 66, y, who, 12, color, 600)
            + text(cx0 + 130, y, said, 13, "#e5e5ea")
        )
        body_lines += f"""<g opacity="0">{shown([(t0, D - 0.4)], D, .25)}
          <animateTransform attributeName="transform" type="translate" dur="{D}s" repeatCount="indefinite" keyTimes="0;{(t0 - .25) / D:.4f};{t0 / D:.4f};1" values="0 6;0 6;0 0;0 0"/>{row}</g>"""
    # the last line arrives as a grey preview, then turns final
    y = 150 + 5 * 32
    t0 = 0.6 + 5 * 1.9
    partial = scene([(t0, t0 + 1.4)], D, text(cx0 + 18, y, "12:42", 11.5, "#636366") + text(cx0 + 66, y, "Me", 12, "#64d2ff", 600)
                    + text(cx0 + 130, y, "Fully local — Whisper runs on the", 13, "#8e8e93", extra=' font-style="italic"'), .2)
    final = scene([(t0 + 1.4, D - 0.4)], D, text(cx0 + 18, y, "12:42", 11.5, "#636366") + text(cx0 + 66, y, "Me", 12, "#64d2ff", 600)
                  + text(cx0 + 130, y, "Fully local — Whisper runs on the GPU, nothing goes to the cloud.", 13, "#e5e5ea"), .05)

    body = f"""{desktop(W, H)}
<path d="{notch_d(CX, PW, PH, 28)}" fill="#000"/>
{topbar(L, "transcribe", timer_frames)}
{card(cx0, cy, cw, ch)}
{head}{body_lines}{partial}{final}"""
    return svg(W, H, body)


# --------------------------------------------------------------------------- 4. Hum's moods


def moods():
    W, H = 900, 190
    items = [("sleep", "Nothing going on", ""), ("idle", "Awake", ""), ("listen", "Recording", ""),
             ("paused", "Paused", ""), ("voice", "Voice prompt", ""), ("work", "Claude working", ""),
             ("happy", "Done", "loop-hop"), ("error", "Something failed", "loop-shake")]
    out = f'<rect width="{W}" height="{H}" rx="18" fill="#111214"/><rect width="{W}" height="{H}" rx="18" fill="url(#peach)"/>'
    step = W / len(items)
    for i, (m, label, cls) in enumerate(items):
        x = step * i + step / 2
        out += hum(x - 39, 42, 78, m, cls=cls) + text(x, 150, label, 12.5, "#c7c7cc", 600, "middle")
    return svg(W, H, WALLPAPER + out)


if __name__ == "__main__":
    OUT.mkdir(exist_ok=True)
    for name, fn in [("notch", hero), ("overview", panel), ("transcribe", transcribe), ("hum", moods)]:
        (OUT / f"{name}.svg").write_text(fn(), encoding="utf-8")
        print("wrote", OUT / f"{name}.svg")
