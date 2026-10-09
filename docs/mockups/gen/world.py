"""Draws the whole invented world as vector elevation bands, equirectangular."""
import random, math, sys
from array import array
from PIL import Image, ImageDraw, ImageFilter

OUT = sys.argv[1]
W, H = 720, 360
SX = 1000 / W


def fnoise(seed, base, octs, pers=0.55):
    rnd = random.Random(seed)
    acc = [0.0] * (W * H)
    amp, tot = 1.0, 0.0
    for o in range(octs):
        gw = base * 2 ** o
        gh = max(2, gw // 2)
        im = Image.frombytes('F', (gw, gh), array('f', [rnd.random() for _ in range(gw * gh)]).tobytes())
        a = array('f', im.resize((W, H), Image.BICUBIC).tobytes())
        acc = [x + amp * y for x, y in zip(acc, a)]
        tot += amp
        amp *= pers
    acc = [x / tot for x in acc]
    m = sum(acc) / len(acc)
    sd = math.sqrt(sum((x - m) ** 2 for x in acc) / len(acc))
    return [min(1.0, max(0.0, 0.5 + (x - m) / sd * 0.2)) for x in acc]


BLOBS = [(18, 44, 30, 20), (-22, 14, 20, 24), (-8, 24, 14, 12), (70, 20, 20, 16), (30, -18, 18, 14), (5, 80, 34, 6),
         (58, 56, 12, 8), (-112, 38, 30, 22), (-96, -24, 20, 26), (140, -30, 26, 14), (150, 36, 16, 18), (-60, -76, 60, 6)]
mk = Image.new('L', (W, H), 0)
d = ImageDraw.Draw(mk)
rb = random.Random(9)
for lon, lat, rx, ry in BLOBS:
    for _ in range(9):
        ox, oy = rb.uniform(-rx, rx) * .75, rb.uniform(-ry, ry) * .75
        ax, ay = rx * rb.uniform(.3, .65), ry * rb.uniform(.3, .65)
        d.ellipse([(lon + ox - ax + 180) * 2, (90 - lat - oy - ay) * 2, (lon + ox + ax + 180) * 2, (90 - lat - oy + ay) * 2], fill=255)
fm = [v / 255 for v in mk.filter(ImageFilter.GaussianBlur(11)).tobytes()]
fw = [v / 255 for v in mk.filter(ImageFilter.GaussianBlur(20)).tobytes()]
n1, n2, n3 = fnoise(21, 6, 5), fnoise(22, 10, 5), fnoise(23, 12, 4, .5)
E = [0.0] * (W * H)
for k in range(W * H):
    f = fm[k] + 1.1 * (n1[k] - 0.5) + 1.0 * (n2[k] - 0.5)
    if f > 0.5:
        u = min(1.0, max(0.03, (fw[k] - 0.4 + 0.35 * (n1[k] - 0.5)) * 1.7))
        r = 1 - abs(2 * n3[k] - 1)
        E[k] = max(4.0, 10 + 300 * u + 800 * u * (n2[k] - 0.5) + 2600 * u * r ** 3 * max(0.0, n1[k] - 0.42) * 2.2)
    else:
        dd = min(1.0, max(0.015, (0.56 - fw[k] - 0.3 * (n1[k] - 0.5)) * 1.9))
        E[k] = -(8 + 2600 * dd ** 1.5)


def bands(level):
    Wp, Hp = W + 2, H + 2

    def v(i, j):
        if i == 0 or j == 0 or i == Wp - 1 or j == Hp - 1:
            return -1e4
        return E[(j - 1) * W + (i - 1)]
    pts, adj = {}, {}

    def pt(key, i1, j1, i2, j2):
        if key not in pts:
            a, b = v(i1, j1), v(i2, j2)
            t = 0.5 if a == b else min(1, max(0, (level - a) / (b - a)))
            pts[key] = ((i1 + (i2 - i1) * t - 0.5) * SX, (j1 + (j2 - j1) * t - 0.5) * SX)
        return key
    TAB = {1: ['LT'], 2: ['TR'], 3: ['LR'], 4: ['RB'], 6: ['TB'], 7: ['LB'], 8: ['BL'], 9: ['TB'],
           11: ['RB'], 12: ['LR'], 13: ['TR'], 14: ['LT']}
    for j in range(Hp - 1):
        r1 = [v(i, j) for i in range(Wp)]
        r2 = [v(i, j + 1) for i in range(Wp)]
        for i in range(Wp - 1):
            idx = (r1[i] >= level) | (r1[i + 1] >= level) << 1 | (r2[i + 1] >= level) << 2 | (r2[i] >= level) << 3
            if idx == 0 or idx == 15:
                continue
            if idx in (5, 10):
                cen = (r1[i] + r1[i + 1] + r2[i] + r2[i + 1]) / 4 >= level
                segs = ['TR', 'BL'] if (idx == 5) == cen else ['LT', 'RB']
            else:
                segs = TAB[idx]
            for s in segs:
                ks = []
                for ch in s:
                    if ch == 'T':
                        ks.append(pt(('h', i, j), i, j, i + 1, j))
                    elif ch == 'R':
                        ks.append(pt(('v', i + 1, j), i + 1, j, i + 1, j + 1))
                    elif ch == 'B':
                        ks.append(pt(('h', i, j + 1), i, j + 1, i + 1, j + 1))
                    else:
                        ks.append(pt(('v', i, j), i, j, i, j + 1))
                adj.setdefault(ks[0], []).append(ks[1])
                adj.setdefault(ks[1], []).append(ks[0])
    seen, out = set(), []
    for start in adj:
        if start in seen:
            continue
        loop, prev, cur = [], None, start
        while cur not in seen:
            seen.add(cur)
            loop.append(pts[cur])
            nb = adj[cur]
            nxt = nb[0] if nb[0] != prev else nb[1]
            if nxt in seen and len(nb) > 1 and nb[1] not in seen:
                nxt = nb[1]
            prev, cur = cur, nxt
        if len(loop) >= 5:
            px, py = round(loop[0][0], 1), round(loop[0][1], 1)
            s = ['M%g,%g' % (px, py)]
            for x, y in loop[1:]:
                x, y = round(x, 1), round(y, 1)
                if x != px or y != py:
                    s.append('l%g,%g' % (round(x - px, 1), round(y - py, 1)))
                    px, py = x, y
            out.append(''.join(s) + 'z')
    return ''.join(out)


SEA_LV = [(-2000, '#a3bfc8'), (-1000, '#b1cace'), (-500, '#c0d5d4'), (-200, '#cfe0d9'), (-50, '#dfebdf')]
LAND_LV = [(0, '#a2b291'), (100, '#b2bf99'), (200, '#c8ce9f'), (400, '#e2dca6'), (700, '#e9d49c'),
           (1000, '#dcb98b'), (1400, '#cc9a7b'), (1800, '#bd7f6e')]
b = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 500">', '<rect width="1000" height="500" fill="#95b3c0"/>']
for lv, col in SEA_LV + LAND_LV:
    dd = bands(lv)
    b.append('<path fill-rule="evenodd" fill="%s" d="%s"/>' % (col, dd))
    if lv == 0:
        coast = dd
b.append('<path fill="none" stroke="#3f8296" stroke-width=".5" stroke-linejoin="round" d="%s"/>' % coast)
g = ''.join('M%.1f,0V500' % (x * 1000 / 12) for x in range(1, 12)) + ''.join('M0,%.1fH1000' % (y * 500 / 6) for y in range(1, 6))
b.append('<path d="%s" fill="none" stroke="#5d6e72" stroke-width=".5" stroke-opacity=".7"/>' % g)
b.append('</svg>')
open(OUT + '/world-base.svg', 'w').write('\n'.join(b))
