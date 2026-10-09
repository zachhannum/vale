"""Draws an original sample atlas plate (invented geography) as two SVG files."""
import random, math, heapq, json, base64, io, sys
from array import array
from PIL import Image, ImageDraw, ImageFilter

OUT = sys.argv[1]
SEED = 11
random.seed(SEED)
PW, PH = 1500, 2000
G = 4
GW, GH = PW // G, PH // G
HW, HH = 750, 1000
FONT = "'Iowan Old Style','Palatino Linotype',Palatino,Georgia,serif"

OUTLINE = [(190, 210), (1330, 210), (1330, 900), (1480, 985), (1480, 1500), (1330, 1585),
           (1330, 1840), (190, 1840), (190, 1000), (20, 915), (20, 480), (190, 395)]
GLOBE = (1300, 1800, 150)


def inside(x, y, poly=OUTLINE):
    c = False
    n = len(poly)
    for a in range(n):
        x1, y1 = poly[a]
        x2, y2 = poly[(a + 1) % n]
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            c = not c
    return c


def usable(x, y, m=28):
    if not all(inside(x + dx, y + dy) for dx in (-m, m) for dy in (-m, m)):
        return False
    return math.hypot(x - GLOBE[0], y - GLOBE[1]) > GLOBE[2] + 30


def fnoise(seed, base, octs, pers=0.5):
    rnd = random.Random(seed)
    acc = [0.0] * (HW * HH)
    amp, tot = 1.0, 0.0
    for o in range(octs):
        gw = base * 2 ** o
        gh = max(2, round(gw * HH / HW))
        im = Image.frombytes('F', (gw, gh), array('f', [rnd.random() for _ in range(gw * gh)]).tobytes())
        a = array('f', im.resize((HW, HH), Image.BICUBIC).tobytes())
        acc = [x + amp * y for x, y in zip(acc, a)]
        tot += amp
        amp *= pers
    acc = [x / tot for x in acc]
    m = sum(acc) / len(acc)
    sd = math.sqrt(sum((x - m) ** 2 for x in acc) / len(acc))
    return [min(1.0, max(0.0, 0.5 + (x - m) / sd * 0.2)) for x in acc]


# ---------- height field ----------
LAND = [(0, 0), (640, 0), (700, 180), (820, 300), (1000, 300), (1120, 380), (1040, 470), (860, 500),
        (740, 600), (700, 760), (800, 860), (980, 900), (1140, 1000), (1250, 1150), (1220, 1300),
        (1080, 1380), (960, 1520), (1000, 1680), (1150, 1800), (1200, 2000), (0, 2000)]
ISLES = [(1190, 570, 62), (1310, 650, 40), (1255, 770, 34), (1110, 690, 24), (1230, 1510, 52),
         (1330, 1430, 30), (1400, 1250, 26), (940, 180, 30)]
RIDGES = [[(150, 350), (350, 300), (520, 420), (600, 600)],
          [(250, 1000), (450, 1100), (600, 1300), (650, 1500)],
          [(150, 1650), (400, 1750), (700, 1850)],
          [(880, 1080), (1020, 1150), (1100, 1250)]]

mk = Image.new('L', (HW, HH), 0)
d = ImageDraw.Draw(mk)
d.polygon([(x / 2, y / 2) for x, y in LAND], fill=255)
for x, y, r in ISLES:
    d.ellipse([(x - r) / 2, (y - r * .8) / 2, (x + r) / 2, (y + r * .8) / 2], fill=255)
fm = [v / 255 for v in mk.filter(ImageFilter.GaussianBlur(26)).tobytes()]
fw = [v / 255 for v in mk.filter(ImageFilter.GaussianBlur(48)).tobytes()]
mt = Image.new('L', (HW, HH), 0)
d = ImageDraw.Draw(mt)
for rg in RIDGES:
    d.line([(x / 2, y / 2) for x, y in rg], fill=255, width=46, joint='curve')
mm = [v / 255 for v in mt.filter(ImageFilter.GaussianBlur(26)).tobytes()]
n1 = fnoise(SEED + 1, 5, 5, .55)
n2 = fnoise(SEED + 2, 8, 5, .55)
n3 = fnoise(SEED + 3, 9, 4, .5)
E = [0.0] * (HW * HH)
for k in range(HW * HH):
    f = fm[k] + 0.75 * (n1[k] - 0.5) + 0.8 * (n2[k] - 0.5)
    if f > 0.5:
        u = min(1.0, max(0.03, (fw[k] - 0.42 + 0.35 * (n1[k] - 0.5)) * 1.7))
        r = 1 - abs(2 * n3[k] - 1)
        e = 10 + 300 * u + 760 * u * (n2[k] - 0.5) + mm[k] * (220 + 1700 * r * r * (0.4 + n2[k]))
        E[k] = max(e, 4.0)
    else:
        dd = min(1.0, max(0.015, (0.56 - fw[k] - 0.3 * (n1[k] - 0.5)) * 1.9))
        E[k] = -(8 + 2600 * dd ** 1.5)
elo = [E[(j * 2) * HW + i * 2] for j in range(GH) for i in range(GW)]


def cell_xy(c):
    return (c % GW) * G + G / 2, (c // GW) * G + G / 2


# ---------- contours ----------
def bands(level):
    W, H = GW + 2, GH + 2
    LOW = -1e9

    def v(i, j):
        if i == 0 or j == 0 or i == W - 1 or j == H - 1:
            return LOW
        return elo[(j - 1) * GW + (i - 1)]
    pts, adj = {}, {}

    def pt(key, i1, j1, i2, j2):
        if key not in pts:
            a, b = v(i1, j1), v(i2, j2)
            a, b = max(a, -1e4), max(b, -1e4)
            t = 0.5 if a == b else min(1, max(0, (level - a) / (b - a)))
            pts[key] = ((i1 + (i2 - i1) * t - 0.5) * G, (j1 + (j2 - j1) * t - 0.5) * G)
        return key
    TAB = {1: ['LT'], 2: ['TR'], 3: ['LR'], 4: ['RB'], 6: ['TB'], 7: ['LB'], 8: ['BL'], 9: ['TB'],
           11: ['RB'], 12: ['LR'], 13: ['TR'], 14: ['LT']}
    for j in range(H - 1):
        row = [v(i, j) >= level for i in range(W)]
        row2 = [v(i, j + 1) >= level for i in range(W)]
        for i in range(W - 1):
            idx = row[i] | row[i + 1] << 1 | row2[i + 1] << 2 | row2[i] << 3
            if idx == 0 or idx == 15:
                continue
            if idx in (5, 10):
                cen = (max(v(i, j), -1e4) + max(v(i + 1, j), -1e4) + max(v(i, j + 1), -1e4) + max(v(i + 1, j + 1), -1e4)) / 4 >= level
                segs = ['TR', 'BL'] if (idx == 5) == cen else ['LT', 'RB']
            else:
                segs = TAB[idx]
            ek = {'T': lambda: pt(('h', i, j), i, j, i + 1, j), 'R': lambda: pt(('v', i + 1, j), i + 1, j, i + 1, j + 1),
                  'B': lambda: pt(('h', i, j + 1), i, j + 1, i + 1, j + 1), 'L': lambda: pt(('v', i, j), i, j, i, j + 1)}
            for s in segs:
                a, b = ek[s[0]](), ek[s[1]]()
                adj.setdefault(a, []).append(b)
                adj.setdefault(b, []).append(a)
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
        if len(loop) >= 6:
            x0, y0 = round(loop[0][0], 1), round(loop[0][1], 1)
            s = ['M%g,%g' % (x0, y0)]
            px, py = x0, y0
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

# ---------- neighbours ----------
NB = [(-1, 0, 1), (1, 0, 1), (0, -1, 1), (0, 1, 1), (-1, -1, 1.414), (1, -1, 1.414), (-1, 1, 1.414), (1, 1, 1.414)]


def neigh(c):
    i, j = c % GW, c // GW
    for dx, dy, w in NB:
        a, b = i + dx, j + dy
        if 0 <= a < GW and 0 <= b < GH:
            yield b * GW + a, w


def chaikin(p, it=2, closed=False):
    for _ in range(it):
        q = [] if closed else [p[0]]
        n = len(p)
        for a in range(n if closed else n - 1):
            x1, y1 = p[a]
            x2, y2 = p[(a + 1) % n]
            q.append((.75 * x1 + .25 * x2, .75 * y1 + .25 * y2))
            q.append((.25 * x1 + .75 * x2, .25 * y1 + .75 * y2))
        if not closed:
            q.append(p[-1])
        p = q
    return p


def smooth(p, step=2, it=2):
    if len(p) > step + 1:
        p = p[:-1:step] + [p[-1]]
    return chaikin(p, it)


def pathd(p, close=False):
    s = 'M%.1f,%.1f' % p[0] + ''.join('L%.1f,%.1f' % q for q in p[1:])
    return s + ('Z' if close else '')


def plen(p):
    return sum(math.hypot(p[a + 1][0] - p[a][0], p[a + 1][1] - p[a][1]) for a in range(len(p) - 1))


# ---------- rivers ----------
jit = [0.8 + 0.4 * random.random() for _ in range(GW * GH)]
dist = [1e18] * (GW * GH)
par = [-1] * (GW * GH)
hq = []
for c in range(GW * GH):
    if elo[c] < 0 and any(elo[n] >= 0 for n, _ in neigh(c)):
        dist[c] = 0
        hq.append((0, c))
heapq.heapify(hq)
while hq:
    dc, c = heapq.heappop(hq)
    if dc > dist[c]:
        continue
    for n, w in neigh(c):
        if elo[n] < 0:
            continue
        nd = dc + w * (1 + elo[n] / 35) * jit[n]
        if nd < dist[n]:
            dist[n] = nd
            par[n] = c
            heapq.heappush(hq, (nd, n))
cands = [c for c in range(GW * GH) if 520 < elo[c] < 1300 and inside(*cell_xy(c))]
random.shuffle(cands)
srcs = []
for c in cands:
    x, y = cell_xy(c)
    if all(math.hypot(x - a, y - b) > 85 for a, b in map(cell_xy, srcs)):
        srcs.append(c)
    if len(srcs) >= 34:
        break
flow = {}
paths = []
for s in srcs:
    p, c = [], s
    while c != -1 and elo[c] >= 0:
        p.append(c)
        c = par[c]
    if c == -1 or len(p) < 12:
        continue
    p.append(c)
    paths.append(p)
    for q in p:
        flow[q] = flow.get(q, 0) + 1
drawn, rivers, mouths, rivlines = set(), [], {}, []
for p in sorted(paths, key=lambda p: -len(p)):
    cut = len(p)
    for a, q in enumerate(p):
        if q in drawn:
            cut = a + 1
            break
    seg = p[:cut]
    drawn.update(p)
    if cut == len(p):
        mouths[p[-2]] = flow[p[-2]]
    sp = smooth([cell_xy(q) for q in seg], 2, 2)
    fl = [flow[seg[min(len(seg) - 1, round(a / (len(sp) - 1) * (len(seg) - 1)))]] for a in range(len(sp))]
    rivlines.append((sp, max(fl), seg))
    a = 0
    while a < len(sp) - 1:
        b = a
        while b < len(sp) - 1 and fl[b] == fl[a]:
            b += 1
        run = sp[a:b + 1]
        w = min(3.6, 1.0 + 0.7 * (fl[a] - 1) ** 0.8)
        if fl[a] == 1 and len(run) > 8:
            h = len(run) // 2
            rivers.append((run[:h + 1], 0.7))
            rivers.append((run[h:], 1.0))
        else:
            rivers.append((run, w))
        a = b
rivercells = {q for q in drawn if elo[q] >= 0}

lakes = []
lk = [q for q in rivercells if 120 < elo[q] < 420 and flow[q] >= 1 and usable(*cell_xy(q))]
random.shuffle(lk)
for q in lk:
    x, y = cell_xy(q)
    if all(math.hypot(x - a, y - b) > 260 for a, b, _ in lakes):
        pq = par[q]
        px, py = cell_xy(pq)
        ang = math.atan2(py - y, px - x)
        L, Wd = random.uniform(22, 40), random.uniform(7, 12)
        pts = []
        for t in range(10):
            th = t / 10 * 2 * math.pi
            rr = random.uniform(.75, 1.2)
            ex, ey = L * math.cos(th) * rr, Wd * math.sin(th) * rr
            pts.append((x + ex * math.cos(ang) - ey * math.sin(ang), y + ex * math.sin(ang) + ey * math.cos(ang)))
        lakes.append((x, y, chaikin(pts, 3, True)))
    if len(lakes) >= 5:
        break

# ---------- cities ----------
NAMES = """Sarn Hollow|Port Callow|Venn|Ilmere|Castellane|Thornwick|Dunmere|Ashby|Greywater|Kessel|Holt|Marrowgate|Tavish|Belmar|Corrow
|Deepwell|Eskar|Fenwick|Garrow|Hartle|Inver|Jessup|Kirrin|Lowmere|Mossgate|Norwell|Penhallow|Quill|Rookhope|Stannick|Tolver|Umber|Varrow
|Wendle|Yarrow|Aldermoor|Brackwater|Caldmoor|Dravin|Elmsworth|Farrowdale|Glaive|Hesper|Istrel|Jorvale|Kelder|Lissom|Merrow|Nantle|Ostrey
|Pellam|Ravenmoor|Selwick|Tarn|Ulver|Wick|Sable Cross|Ardent|Brenn|Coldharbor|Dunmarrow|Evenwood|Fallow|Gant""".replace('\n', '').split('|')
nf = fnoise(SEED + 4, 3, 2)


def foreign(c):
    x, y = cell_xy(c)
    return x + 900 * (nf[int(y / 2) * HW + int(x / 2)] - 0.5) < 400


cities = []


def far(c, sep):
    x, y = cell_xy(c)
    return all(math.hypot(x - a['x'], y - a['y']) > sep for a in cities)


def add(c, port=False, rank=3):
    x, y = cell_xy(c)
    cities.append({'c': c, 'x': x, 'y': y, 'port': port, 'rank': rank, 'name': NAMES[len(cities)], 'foreign': foreign(c)})


ms = sorted(mouths, key=lambda c: -mouths[c])
for c in ms:
    if usable(*cell_xy(c)) and far(c, 95) and len(cities) < 9:
        add(c, True, 2)
coast = [c for c in range(GW * GH) if elo[c] >= 0 and any(elo[n] < 0 for n, w in neigh(c) if w == 1) and usable(*cell_xy(c))]
random.shuffle(coast)
for c in coast:
    if far(c, 100) and len(cities) < 19:
        add(c, random.random() < .3)
rv = [c for c in rivercells if elo[c] < 620 and usable(*cell_xy(c))]
random.shuffle(rv)
k0 = len(cities)
for c in rv:
    if far(c, 84) and len(cities) < k0 + 20:
        add(c)
low = [c for c in range(GW * GH) if 0 <= elo[c] < 520 and usable(*cell_xy(c))]
random.shuffle(low)
for c in low:
    if far(c, 84) and len(cities) < 58:
        add(c)
dom = [a for a in cities if not a['foreign']]
cap = next(a for a in cities if a['port'] and not a['foreign'])
cap['rank'] = 1
for a in random.sample([a for a in dom if a is not cap and not a['port']], 5):
    a['rank'] = 2

# ---------- provinces ----------
seeds = [cap]
while len(seeds) < 6:
    seeds.append(max(dom, key=lambda a: min(math.hypot(a['x'] - s['x'], a['y'] - s['y']) for s in seeds)))
pid = [None] * (GW * GH)
pd = [1e18] * (GW * GH)
hq = []
for s, a in enumerate(seeds):
    pd[a['c']] = 0
    pid[a['c']] = s
    hq.append((0, a['c'], s))
while hq:
    dc, c, s = heapq.heappop(hq)
    if dc > pd[c]:
        continue
    for n, w in neigh(c):
        if elo[n] < 0 or foreign(n):
            continue
        nd = dc + w * (1 + elo[n] / 180)
        if nd < pd[n]:
            pd[n] = nd
            pid[n] = s
            heapq.heappush(hq, (nd, n, s))
for c in range(GW * GH):
    if elo[c] >= 0 and foreign(c):
        pid[c] = -2


def chain(segs):
    adj = {}
    for a, b in segs:
        adj.setdefault(a, set()).add(b)
        adj.setdefault(b, set()).add(a)
    used, out = set(), []

    def walk(a, b):
        p = [a, b]
        used.add((a, b)); used.add((b, a))
        while len(adj[p[-1]]) == 2:
            nx = [q for q in adj[p[-1]] if q != p[-2]][0]
            if (p[-1], nx) in used:
                break
            used.add((p[-1], nx)); used.add((nx, p[-1]))
            p.append(nx)
        return p
    for a in adj:
        if len(adj[a]) != 2:
            for b in adj[a]:
                if (a, b) not in used:
                    out.append(walk(a, b))
    for a in adj:
        for b in adj[a]:
            if (a, b) not in used:
                out.append(walk(a, b))
    return out


bseg = {'nat': [], 'prov': []}
for j in range(GH - 1):
    for i in range(GW - 1):
        c = j * GW + i
        for n, seg in ((c + 1, ((i + 1, j), (i + 1, j + 1))), (c + GW, ((i, j + 1), (i + 1, j + 1)))):
            a, b = pid[c], pid[n]
            if a is None or b is None or a == b:
                continue
            bseg['nat' if -2 in (a, b) else 'prov'].append(seg)
borders = {k: [smooth([(x * G, y * G) for x, y in p], 3, 2) for p in chain(v) if len(p) > 6] for k, v in bseg.items()}


# ---------- roads and sea routes ----------
def route(a, b, cost, ok, pad):
    i1, j1, i2, j2 = a % GW, a // GW, b % GW, b // GW
    lo_i, hi_i, lo_j, hi_j = min(i1, i2) - pad, max(i1, i2) + pad, min(j1, j2) - pad, max(j1, j2) + pad
    ds, pr, hq = {a: 0}, {}, [(0, a)]
    while hq:
        dc, c = heapq.heappop(hq)
        if c == b:
            p = [b]
            while p[-1] != a:
                p.append(pr[p[-1]])
            return p[::-1]
        if dc > ds[c]:
            continue
        for n, w in neigh(c):
            if not (lo_i <= n % GW <= hi_i and lo_j <= n // GW <= hi_j) or not ok(n):
                continue
            nd = dc + w * cost(c, n)
            if nd < ds.get(n, 1e18):
                ds[n] = nd
                pr[n] = c
                heapq.heappush(hq, (nd, n))
    return None


roadcell = set()
edges = set()
for a in cities:
    near = sorted((b for b in cities if b is not a), key=lambda b: math.hypot(a['x'] - b['x'], a['y'] - b['y']))[:2 if a['rank'] == 3 else 3]
    for b in near:
        edges.add(tuple(sorted((a['c'], b['c']))))
roads = []
for a, b in sorted(edges):
    p = route(a, b, lambda c, n: (1 + elo[n] / 300 + abs(elo[n] - elo[c]) / 25) * (0.4 if n in roadcell else 1) * jit[n], lambda n: elo[n] >= 0, 30)
    if p:
        roadcell.update(p)
        roads.append(smooth([cell_xy(q) for q in p], 5, 3))
ports = [a for a in cities if a['port']]
sedges = set()
for a in ports:
    near = sorted((b for b in ports if b is not a), key=lambda b: math.hypot(a['x'] - b['x'], a['y'] - b['y']))
    for b in near[:1]:
        sedges.add(tuple(sorted((a['name'], b['name']))))
farp = max(ports, key=lambda b: math.hypot(cap['x'] - b['x'], cap['y'] - b['y']))
sedges.add(tuple(sorted((cap['name'], farp['name']))))
byname = {a['name']: a for a in cities}
searoutes = []
for an, bn in sorted(sedges):
    a, b = byname[an], byname[bn]
    sa = next((n for n, _ in neigh(a['c']) if elo[n] < 0), None)
    sb = next((n for n, _ in neigh(b['c']) if elo[n] < 0), None)
    if sa is None or sb is None:
        continue
    p = route(sa, sb, lambda c, n: 1 + 500 / (12 + -elo[n]), lambda n: elo[n] < 0, 90)
    if p and len(p) > 10:
        pts = [(a['x'], a['y'])] + smooth([cell_xy(q) for q in p], 7, 3) + [(b['x'], b['y'])]
        if all(inside(x, y) for x, y in pts[::4]):
            searoutes.append((an + '–' + bn, pts))

# ---------- hillshade ----------
sh = bytearray(HW * HH)
Z = 1 / 70.0
lx, ly, lz = -0.55, -0.55, 0.63
for j in range(HH):
    for i in range(HW):
        k = j * HW + i
        e = E[k]
        if e < 0 or i == 0 or j == 0 or i == HW - 1 or j == HH - 1:
            sh[k] = 255
            continue
        dx = (max(E[k + 1], 0) - max(E[k - 1], 0)) * Z
        dy = (max(E[k + HW], 0) - max(E[k - HW], 0)) * Z
        s = (-dx * lx - dy * ly + lz) / math.sqrt(dx * dx + dy * dy + 1) / lz
        sh[k] = int(255 * min(1.0, max(0.62, 0.36 + 0.62 * s)))
shim = Image.frombytes('L', (HW, HH), bytes(sh)).resize((HW * 2, HH * 2), Image.BICUBIC).filter(ImageFilter.GaussianBlur(1.2))
buf = io.BytesIO()
shim.save(buf, 'JPEG', quality=72)
shade64 = base64.b64encode(buf.getvalue()).decode()

# ---------- projection ----------
N0 = math.sin(math.radians(46))
R0, CX, CY, DEG = 6546.0, 760.0, 1025.0, 118.3


def proj(lon, lat):
    r = R0 - (lat - 46) * DEG
    th = N0 * math.radians(lon - 30)
    return CX + r * math.sin(th), CY - R0 + r * math.cos(th)


grat, glabels = [], []
for lon in range(16, 46, 2):
    p = [proj(lon, 38 + t * 0.25) for t in range(0, 69)]
    grat.append(p)
    for a in range(len(p) - 1):
        if inside(*p[a]) != inside(*p[a + 1]) and 21 < lon < 39:
            x, y = p[a]
            glabels.append((x, y - 9 if y < CY else y + 18, 'middle', '%d°E' % lon))
for lat in range(38, 56, 2):
    p = [proj(14 + t * 0.25, lat) for t in range(0, 129)]
    grat.append(p)
    for a in range(len(p) - 1):
        if inside(*p[a]) != inside(*p[a + 1]):
            x, y = p[a]
            glabels.append((x - 7 if x < CX else x + 7, y + 4, 'end' if x < CX else 'start', '%d°N' % lat))

# ---------- base svg ----------
PAPER = '#f3efdf'
op = pathd(OUTLINE, True)
b = ['<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xml:space="preserve" viewBox="0 0 %d %d">' % (PW, PH),
     '<defs><clipPath id="o"><path d="%s"/></clipPath></defs>' % op,
     '<g clip-path="url(#o)">', '<rect width="%d" height="%d" fill="#95b3c0"/>' % (PW, PH)]
for lv, col in SEA_LV:
    b.append('<path fill-rule="evenodd" fill="%s" d="%s"/>' % (col, bands(lv)))
coastd = None
for lv, col in LAND_LV:
    dd = bands(lv)
    if lv == 0:
        coastd = dd
    b.append('<path fill-rule="evenodd" fill="%s" d="%s"/>' % (col, dd))
b.append('<path fill="none" stroke="#3f8296" stroke-width=".9" stroke-linejoin="round" d="%s"/>' % coastd)
b.append('<g fill="none" stroke="#3f8aa3" stroke-linecap="round" stroke-linejoin="round">')
for p, w in rivers:
    b.append('<path stroke-width="%.2f" d="%s"/>' % (w, pathd(p)))
b.append('</g><g fill="#d3e6e0" stroke="#3f8aa3" stroke-width=".8">')
for _, _, p in lakes:
    b.append('<path d="%s"/>' % pathd(p, True))
b.append('</g><g fill="none" stroke="#5d6e72" stroke-width=".7" stroke-opacity=".75">')
for p in grat:
    b.append('<path d="%s"/>' % pathd(p))
b.append('</g><g fill="none" stroke-linejoin="round" stroke-linecap="round">')
for p in borders['nat']:
    b.append('<path stroke="#c98f88" stroke-opacity=".75" stroke-width="9" d="%s"/>' % pathd(p))
for p in borders['nat']:
    b.append('<path stroke="#b5483c" stroke-width="1.5" stroke-dasharray="9 3 2 3" d="%s"/>' % pathd(p))
for p in borders['prov']:
    b.append('<path stroke="#bf5645" stroke-width="1.3" stroke-dasharray="6 4" d="%s"/>' % pathd(p))
for p in roads:
    b.append('<path stroke="#86503f" stroke-width="1.05" d="%s"/>' % pathd(p))
for _, p in searoutes:
    b.append('<path stroke="#2c5866" stroke-width="1.2" stroke-dasharray="9 5" d="%s"/>' % pathd(p))
b.append('</g></g>')
b.append('<path d="%s" fill="none" stroke="#4a4a44" stroke-width="1.2"/>' % op)
b.append('</svg>')
open(OUT + '/plate-base.svg', 'w').write('\n'.join(b))
open(OUT + '/page-under.svg', 'w').write('\n'.join([b[0], '<rect width="%d" height="%d" fill="%s"/>' % (PW, PH, PAPER),
    '<rect x="150" y="170" width="1220" height="1710" fill="none" stroke="#c9aaa0" stroke-width="13"/>',
    '<path d="%s" fill="%s" stroke="%s" stroke-width="26"/>' % (op, PAPER, PAPER), '</svg>']))
sk = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="200 880 900 686"><rect x="0" y="0" width="1500" height="2000" fill="#f4f1e6"/>',
      '<path fill="#ebe6d6" fill-rule="evenodd" stroke="#3b3b3b" stroke-width="2.2" stroke-linejoin="round" d="%s"/>' % coastd,
      '<g fill="none" stroke="#5a5a5a" stroke-linecap="round">'] + ['<path stroke-width="%.2f" d="%s"/>' % (w * 1.2, pathd(p)) for p, w in rivers] + ['</g></svg>']
open(OUT + '/../gen/sketch.svg', 'w').write('\n'.join(sk))

# ---------- labels ----------
occ = []


def hit(bx):
    return any(bx[0] < o[2] and o[0] < bx[2] and bx[1] < o[3] and o[1] < bx[3] for o in occ)


def sc(text, s, extra=''):
    out = []
    for w in text.split(' '):
        out.append('%s<tspan font-size="%.1f">%s</tspan>' % (w[0], s * .78, w[1:].upper()))
    return ' '.join(out)


def scw(text, s):
    return sum(.70 * s + (len(w) - 1) * .78 * s * .72 for w in text.split(' ')) + .3 * s * text.count(' ')


L = ['<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xml:space="preserve" viewBox="0 0 %d %d" font-family="%s">' % (PW, PH, FONT), '<defs>']
T = []
defs = []
SIZES = {1: 16, 2: 13, 3: 10.5}
RAD = {1: 5, 2: 4, 3: 3.2}
for a in cities:
    r = RAD[a['rank']]
    occ.append((a['x'] - r - 1, a['y'] - r - 1, a['x'] + r + 1, a['y'] + r + 1))
    if a['port']:
        occ.append((a['x'] - 6, a['y'] - r - 15, a['x'] + 6, a['y'] - r - 2))
unplaced = []
for a in sorted(cities, key=lambda a: a['rank']):
    s, r, x, y = SIZES[a['rank']], RAD[a['rank']], a['x'], a['y']
    w = scw(a['name'], s)
    cand = [('NE', x + r + 3, y - r - 1, 0), ('SE', x + r + 3, y + r + s * .7, 0), ('E', x + r + 4, y + s * .33, 0),
            ('NW', x - r - 3, y - r - 1, 1), ('SW', x - r - 3, y + r + s * .7, 1), ('W', x - r - 4, y + s * .33, 1),
            ('S', x, y + r + s * .95, .5), ('N', x, y - r - 3, .5)]
    a['cands'] = []
    done = False
    for nm, tx, ty, an in cand:
        bx = (tx - w * an - 1, ty - s * .78, tx - w * an + w + 1, ty + s * .16)
        ok = not hit(bx) and all(inside(px, py) for px in (bx[0], bx[2]) for py in (bx[1], bx[3])) \
            and math.hypot((bx[0] + bx[2]) / 2 - GLOBE[0], (bx[1] + bx[3]) / 2 - GLOBE[1]) > GLOBE[2] + 30
        a['cands'].append((nm, [round(v, 1) for v in bx], ok))
        if ok and not done:
            done = True
            occ.append(bx)
            a['box'] = [round(v, 1) for v in bx]
            a['pos'] = nm
            T.append('<text x="%.1f" y="%.1f" font-size="%g" text-anchor="%s"%s>%s</text>' % (
                tx, ty, s, ('start', 'end', 'middle')[0 if an == 0 else 1 if an == 1 else 2],
                ' font-weight="bold"' if a['rank'] == 1 else '', sc(a['name'], s)))
    if not done:
        unplaced.append(a['name'])
S = []
for a in cities:
    r = RAD[a['rank']]
    S.append('<circle cx="%.1f" cy="%.1f" r="%g" fill="%s" stroke="#5a3229" stroke-width="1.1"/>' % (a['x'], a['y'], r, PAPER))
    if a['rank'] < 3:
        S.append('<circle cx="%.1f" cy="%.1f" r="%g" fill="#5a3229"/>' % (a['x'], a['y'], r * .38))
    if a['port']:
        S.append('<path transform="translate(%.1f %.1f)" d="M0,-11V0M-3,-8.500H3M-4.500,-3.500C-4,0.500 4,0.500 4.500,-3.500" fill="none" stroke="#8c2f24" stroke-width="1.2" stroke-linecap="round"/><circle cx="%.1f" cy="%.1f" r="1.4" fill="none" stroke="#8c2f24" stroke-width="1"/>' % (a['x'], a['y'] - r - 3, a['x'], a['y'] - r - 15.5))


def spaced(text, x1, y1, x2, y2, bow, size, fill, italic=False, key=None, opacity=1, maxls=60):
    key = key or 'p%d' % len(defs)
    mx, my = (x1 + x2) / 2, (y1 + y2) / 2
    ln = math.hypot(x2 - x1, y2 - y1)
    want = (len(text) * size * .74 + (len(text) - 1) * maxls) / .88
    if ln > want:
        k = want / ln / 2
        x1, y1, x2, y2 = mx - (x2 - x1) * k, my - (y2 - y1) * k, mx + (x2 - x1) * k, my + (y2 - y1) * k
        ln = want
    nx, ny = -(y2 - y1) / ln, (x2 - x1) / ln
    defs.append('<path id="%s" d="M%.1f,%.1f Q%.1f,%.1f %.1f,%.1f"/>' % (key, x1, y1, mx + nx * bow * 2, my + ny * bow * 2, x2, y2))
    n = len(text)
    ls = max(1, (ln * .88 - n * size * .74) / max(1, n - 1))
    return '<text font-size="%g" letter-spacing="%.1f" fill="%s" fill-opacity="%g"%s><textPath xlink:href="#%s">%s<tspan font-size="%g">%s</tspan></textPath></text>' % (
        size * 1.3, ls, fill, opacity, ' font-style="italic"' if italic else '', key, text[0], size, text[1:])


BIG = []
PNAMES = ['MARROWMARK', 'HALDANE', 'OSTRAVELLE', 'GREYWOOD', 'SABLEMOOR', 'CALLOWAY']
pinfo = []
for s in range(len(seeds)):
    cs = [c for c in range(0, GW * GH, 3) if pid[c] == s and inside(*cell_xy(c))]
    if len(cs) < 60:
        continue
    xs = sorted(cell_xy(c)[0] for c in cs)
    ys = sorted(cell_xy(c)[1] for c in cs)
    mx, my = xs[len(xs) // 2], ys[len(ys) // 2]
    wd = (xs[int(len(xs) * .9)] - xs[int(len(xs) * .1)]) * .8
    wd = max(170, min(wd, 420))
    tilt = random.uniform(-18, 18)
    BIG.append(spaced(PNAMES[s], mx - wd / 2, my - tilt, mx + wd / 2, my + tilt, random.choice((-10, 10)), 17, '#3a3630', opacity=.9, maxls=24))
    pinfo.append((PNAMES[s], round(mx), round(my)))
dc = [cell_xy(c) for c in range(0, GW * GH, 5) if pid[c] is not None and pid[c] >= 0 and inside(*cell_xy(c))]
dmx = sorted(p[0] for p in dc)[len(dc) // 2]
dmy = sorted(p[1] for p in dc)[len(dc) // 2]
BIG.append(spaced('VESPERAN', dmx - 330, dmy + 40, dmx + 330, dmy + 10, -18, 34, '#2e2b27'))
fc = [cell_xy(c) for c in range(0, GW * GH, 5) if pid[c] == -2 and inside(*cell_xy(c))]
if fc:
    fy = sorted(p[1] for p in fc)
    fx = sorted(p[0] for p in fc)[len(fc) // 2]
    BIG.append(spaced('DRAVENHOLD', fx - 40, fy[int(len(fy) * .82)], fx + 30, fy[int(len(fy) * .22)], 14, 24, '#3a3630'))
SEAS = json.load(open(OUT + '/../gen/seas.json'))
for t, x1, y1, x2, y2, bow, size in SEAS:
    BIG.append(spaced(t, x1, y1, x2, y2, bow, size, '#2f5f73', italic=True))
RNAMES = ['HALDA MOUNTAINS', 'THE GREY TEETH', 'SOUTHERN FELLS', 'CALLOW HILLS']
for rg, nm in zip(RIDGES, RNAMES):
    (x1, y1), (x2, y2) = rg[0], rg[-1]
    if x1 > x2:
        x1, y1, x2, y2 = x2, y2, x1, y1
    mxr, myr = rg[len(rg) // 2]
    bow = ((myr - (y1 + y2) / 2)) * .5
    BIG.append(spaced(nm, x1 + 20, y1, x2 - 20, y2, bow * .6, 13, '#3a3630', maxls=13))
RIVN = ['Aldwen', 'Sable', 'Meriden', 'Tarrow', 'Ysel', 'Corr']
RT = []
for (sp, fmax, seg), nm in zip(sorted([r for r in rivlines if plen(r[0]) > 230], key=lambda r: -plen(r[0])), RIVN):
    n = len(sp)
    part = sp[int(n * .38):int(n * .78)]
    if part[0][0] > part[-1][0]:
        part = part[::-1]
    part = chaikin(part[::4] + [part[-1]], 2)
    if not all(usable(x, y, 12) for x, y in part):
        continue
    key = 'r%d' % len(defs)
    defs.append('<path id="%s" d="%s"/>' % (key, pathd(part)))
    RT.append('<text dy="-3.500"><textPath xlink:href="#%s" startOffset="6%%">%s R.</textPath></text>' % (key, nm))
for k, (nm, p) in enumerate(searoutes):
    if plen(p) < 240:
        continue
    n = len(p)
    part = p[int(n * .3):int(n * .75)]
    if part[0][0] > part[-1][0]:
        part = part[::-1]
    key = 's%d' % k
    defs.append('<path id="%s" d="%s"/>' % (key, pathd(part)))
    RT.append('<text dy="-4"><textPath xlink:href="#%s" startOffset="10%%">%s</textPath></text>' % (key, nm))
PK = []
pk = sorted((c for c in range(GW * GH) if elo[c] > 1500 and usable(*cell_xy(c)) and all(elo[n] <= elo[c] for n, _ in neigh(c))), key=lambda c: -elo[c])
PKN = ['Mt. Orrin', 'Halda', 'Greyhorn', 'Skarn', 'Tolfell', 'Brannoch']
pks = []
for c in pk:
    x, y = cell_xy(c)
    if all(math.hypot(x - a, y - b2) > 150 for a, b2 in pks) and len(pks) < 6:
        bx = (x + 5, y - 8, x + 78, y + 4)
        if hit(bx):
            continue
        occ.append(bx)
        pks.append((x, y))
        PK.append('<path d="M%.1f,%.1f l4,-7 l4,7z" fill="#4a3a32"/><text x="%.1f" y="%.1f">%s %s</text>' % (
            x - 4, y + 3, x + 7, y + 3, PKN[len(pks) - 1], format(int(elo[c]), ',')))

L += defs
L.append('</defs>')
L.append('<g>' + ''.join(BIG) + '</g>')
L.append('<g font-size="9.500" font-style="italic" fill="#2b6a82">' + ''.join(RT) + '</g>')
L.append('<g font-size="9.500" font-style="italic" fill="#3a302a">' + ''.join(PK) + '</g>')
L.append(''.join(S))
L.append('<g fill="#2a2420">' + ''.join(T) + '</g>')
L.append('<g font-size="12" fill="#3c3a35">' + ''.join('<text x="%.1f" y="%.1f" text-anchor="%s">%s</text>' % g for g in glabels) + '</g>')
# furniture
LMAP = len(L)
L.append('<text x="150" y="146" font-size="27" fill="#3a3630">A<tspan font-size="21">TLAS OF </tspan>K<tspan font-size="21">ARETH</tspan></text>')
L.append('<text x="1370" y="146" font-size="27" fill="#3a3630" text-anchor="end">V<tspan font-size="21">ESPERAN</tspan></text>')
L.append('<text x="150" y="1918" font-size="19" fill="#3a3630">M<tspan font-size="15">ERIDIAN </tspan>S<tspan font-size="15">URVEY</tspan></text>')
L.append('<text x="430" y="1946" font-size="14" fill="#3a3630" text-anchor="middle">S<tspan font-size="11">CALE</tspan> 1:3,500,000</text>')
KM, MI = 1 / 0.7112, 1.609 / 0.7112
sb = ['<g stroke="#3a3630" stroke-width="1" fill="none">', '<path d="M250,1968H%.1f"/>' % (250 + 240 * KM)]
for v in (0, 15, 30, 60, 90, 120, 150):
    sb.append('<path d="M%.1f,1968v-6"/>' % (250 + v * MI))
for v in (0, 30, 60, 120, 180, 240):
    sb.append('<path d="M%.1f,1968v6"/>' % (250 + v * KM))
sb.append('</g><g font-size="9.500" fill="#3a3630" text-anchor="middle">')
for v in (0, 15, 30, 60, 90, 120, 150):
    sb.append('<text x="%.1f" y="1958">%d</text>' % (250 + v * MI, v))
for v in (0, 30, 60, 120, 180, 240):
    sb.append('<text x="%.1f" y="1985">%d</text>' % (250 + v * KM, v))
sb.append('<text x="%.1f" y="1958" text-anchor="start">MILES</text><text x="%.1f" y="1985" text-anchor="start">KILOMETERS</text></g>' % (250 + 150 * MI + 14, 250 + 240 * KM + 30))
L += sb
# globe
gx, gy, gr = GLOBE
g0, p0 = math.radians(24), math.radians(30)


def orth(lon, lat):
    lo, la = math.radians(lon) - g0, math.radians(lat)
    cz = math.sin(p0) * math.sin(la) + math.cos(p0) * math.cos(la) * math.cos(lo)
    return gx + gr * math.cos(la) * math.sin(lo), gy - gr * (math.cos(p0) * math.sin(la) - math.sin(p0) * math.cos(la) * math.cos(lo)), cz > 0


def gline(pts):
    out, cur = [], []
    for lon, lat in pts:
        x, y, v = orth(lon, lat)
        if v:
            cur.append((x, y))
        else:
            if len(cur) > 1:
                out.append(pathd(cur))
            cur = []
    if len(cur) > 1:
        out.append(pathd(cur))
    return ''.join(out)


gl = ['<circle cx="%d" cy="%d" r="%d" fill="#b9d2d6" stroke="#4a4a44" stroke-width="1.5"/>' % GLOBE, '<clipPath id="gc"><circle cx="%d" cy="%d" r="%d"/></clipPath><g clip-path="url(#gc)">' % GLOBE]
grnd = random.Random(5)
for clon, clat, rad in ((16, 44, 30), (-22, 18, 15), (68, 22, 15), (30, -14, 15), (5, 76, 10), (55, 55, 9)):
    pts = []
    for t in range(40):
        th = t / 40 * 2 * math.pi
        rr = rad * (0.85 + 0.12 * math.sin(3 * th + clon) + 0.1 * math.sin(5 * th + clat) + 0.08 * grnd.random())
        la = clat + rr * math.sin(th)
        pts.append(orth(clon + rr * math.cos(th) / max(.3, math.cos(math.radians(max(-80, min(80, la))))), max(-88, min(88, la)))[:2])
    gl.append('<path d="%s" fill="#a9b79a" stroke="#3f8296" stroke-width=".8"/>' % pathd(chaikin(pts, 2, True), True))
box = [(23 + t, 39) for t in range(15)] + [(37, 39 + t) for t in range(15)] + [(37 - t, 53) for t in range(15)] + [(23, 53 - t) for t in range(15)]
gl.append('<path d="%s" fill="#c8594a" fill-opacity=".85" stroke="#8c2f24" stroke-width="1"/>' % pathd([orth(a, b2)[:2] for a, b2 in box], True))
gl.append('<g fill="none" stroke="#5d6e72" stroke-width=".8">')
for lon in range(-180, 180, 30):
    gl.append('<path d="%s"/>' % gline([(lon, la) for la in range(-90, 91, 3)]))
for lat in range(-60, 90, 30):
    gl.append('<path d="%s"/>' % gline([(lo, lat) for lo in range(-180, 181, 3)]))
gl.append('</g></g>')
L += gl
L.append('</svg>')
open(OUT + '/plate-labels.svg', 'w').write('\n'.join(L[:LMAP] + ['</svg>']))
open(OUT + '/page-over.svg', 'w').write('\n'.join([L[0]] + L[LMAP:]))
both = '\n'.join(b[:-1]) + '\n' + '\n'.join(L[1:])
open(OUT + '/../gen/preview.svg', 'w').write(both.replace('<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xml:space="preserve" viewBox="0 0 %d %d">' % (PW, PH), '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xml:space="preserve" viewBox="0 0 %d %d" font-family="%s">' % (PW, PH, FONT), 1))
json.dump({'cities': [{k: v for k, v in a.items() if k != 'c'} for a in cities], 'unplaced': unplaced, 'provinces': pinfo,
           'lakes': [(round(x), round(y)) for x, y, _ in lakes], 'peaks': pks, 'routes': [r[0] for r in searoutes]},
          open(OUT + '/../gen/info.json', 'w'))
print('cities', len(cities), 'unplaced', unplaced, 'rivers', len(paths), 'roads', len(roads), 'routes', [r[0] for r in searoutes])
