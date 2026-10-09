"""Draws the invented world on an orthographic globe, as vector elevation bands."""
import sys, math, importlib.util
spec = importlib.util.spec_from_file_location('world', 'gen/world.py')
w = importlib.util.module_from_spec(spec)
spec.loader.exec_module(w)
OUT = sys.argv[1]
WW, WH, WE = w.W, w.H, w.E
N = 500
L0, P0 = math.radians(18), math.radians(14)


def sample(lon, lat):
    fx = ((lon + 180) % 360) * 2 - 0.5
    fy = min(WH - 1.001, max(0, (90 - lat) * 2 - 0.5))
    x0, y0 = int(math.floor(fx)), int(fy)
    tx, ty = fx - x0, fy - y0
    a, b = WE[y0 * WW + x0 % WW], WE[y0 * WW + (x0 + 1) % WW]
    c, d = WE[(y0 + 1) * WW + x0 % WW], WE[(y0 + 1) * WW + (x0 + 1) % WW]
    return (a * (1 - tx) + b * tx) * (1 - ty) + (c * (1 - tx) + d * tx) * ty


G = [0.0] * (N * N)
for j in range(N):
    for i in range(N):
        x, y = (i + 0.5) / N * 2 - 1, 1 - (j + 0.5) / N * 2
        rho = math.hypot(x, y)
        if rho > 0.999:
            x, y, rho = x / rho * 0.999, y / rho * 0.999, 0.999
        c = math.asin(rho)
        lat = math.asin(max(-1, min(1, math.cos(c) * math.sin(P0) + (y * math.sin(c) * math.cos(P0) / rho if rho else 0))))
        lon = L0 + math.atan2(x * math.sin(c), rho * math.cos(c) * math.cos(P0) - y * math.sin(c) * math.sin(P0))
        G[j * N + i] = sample(math.degrees(lon), math.degrees(lat))
w.W, w.H, w.E, w.SX = N, N, G, 1000 / N


def fwd(lon, lat):
    l, p = math.radians(lon) - L0, math.radians(lat)
    if math.sin(P0) * math.sin(p) + math.cos(P0) * math.cos(p) * math.cos(l) < 0.02:
        return None
    return 500 + 496 * math.cos(p) * math.sin(l), 500 - 496 * (math.cos(P0) * math.sin(p) - math.sin(P0) * math.cos(p) * math.cos(l))


def line(pts):
    d, pen = '', False
    for q in pts:
        if q is None:
            pen = False
        else:
            d += ('L' if pen else 'M') + '%.1f,%.1f' % q
            pen = True
    return d


g = ''.join(line([fwd(lon, t) for t in range(-90, 91, 3)]) for lon in range(-180, 180, 30))
g += ''.join(line([fwd(t, lat) for t in range(-180, 181, 3)]) for lat in range(-60, 61, 30))
for name, sea, land, coastc in (('globe', w.SEA_LV, w.LAND_LV, '#3f8296'),):
    b = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 1000">', '<clipPath id="c"><circle cx="500" cy="500" r="496"/></clipPath>',
         '<g clip-path="url(#c)"><rect width="1000" height="1000" fill="#95b3c0"/>']
    for lv, col in sea + land:
        dd = w.bands(lv)
        b.append('<path fill-rule="evenodd" fill="%s" d="%s"/>' % (col, dd))
        if lv == 0:
            coast = dd
    b.append('<path fill="none" stroke="%s" stroke-width=".8" stroke-linejoin="round" d="%s"/>' % (coastc, coast))
    b.append('<path d="%s" fill="none" stroke="#5d6e72" stroke-width=".7" stroke-opacity=".6"/></g></svg>' % g)
    open('%s/%s.svg' % (OUT, name), 'w').write('\n'.join(b))
