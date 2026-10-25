"""Writes the screen files: the iPad boards, then the desktop boards."""
import sys

OUT = sys.argv[1]
ON = ' checked="{{ true }}"'


def field(label, value, mono=False):
    return '<div class="row"><span class="lab">%s</span><input class="in%s" value="%s" aria-label="%s"></div>' % (
        label, ' mono' if mono else '', value, label)


def pick(label, *opts):
    return '<div class="row"><span class="lab">%s</span><select class="in" aria-label="%s">%s</select></div>' % (
        label, label, ''.join('<option>%s</option>' % o for o in opts))


def check(label, on=True):
    return '<label class="row"><input class="chk" type="checkbox"%s><span>%s</span></label>' % (ON if on else '', label)


def item(name, right='', sel=False, sw=None, cls='mute'):
    s = '<span class="sw" style="background: %s"></span>' % sw if sw else ''
    r = '<span class="%s">%s</span>' % (cls, right) if right else ''
    return '<div class="row%s">%s<span style="flex: 1">%s</span>%s</div>' % (' sel' if sel else '', s, name, r)


TOOLS = (('Select', '<path d="M5 3l14 8-6 2-2 6z"></path>'),
         ('Pan', '<path d="M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3"></path>'),
         ('Place point', '<path d="M12 21s-6-6-6-11a6 6 0 0112 0c0 5-6 11-6 11z"></path><circle cx="12" cy="10" r="2"></circle>'),
         ('Edit vertices', '<path d="M5 18l5-10 5 6 4-8"></path><circle cx="5" cy="18" r="1.5"></circle><circle cx="10" cy="8" r="1.5"></circle><circle cx="15" cy="14" r="1.5"></circle>'),
         ('Edit labels', '<path d="M6 6h12M12 6v13M9 19h6"></path>'))
DTOOLS = (('Raise', '<path d="M3 18c4 0 5-11 9-11s5 11 9 11"></path>'), ('Lower', '<path d="M3 7c4 0 5 11 9 11s5-11 9-11"></path>'),
          ('Smooth', '<path d="M3 12c3-4 6-4 9 0s6 4 9 0"></path>'), ('Flatten', '<path d="M4 12h16"></path>'),
          ('Line', '<path d="M4 20l4-1 11-11-3-3L5 16z"></path>'), TOOLS[1])

PLATE = '<img class="lay" src="./plate-base.svg" alt="%s"><img class="lay" src="./plate-labels.svg" alt="">'
FULL = '<img class="lay" src="./page-under.svg" alt=""><img class="lay" src="./plate-base.svg" alt="%s"><img class="lay" src="./plate-labels.svg" alt=""><img class="lay" src="./page-over.svg" alt="">'
GLOBE = ('<img class="lay" src="./globe.svg" alt="Globe of Kareth with elevation bands">'
         '<svg class="lay" viewBox="0 0 1000 1000"><circle cx="388" cy="402" r="46" fill="none" stroke="#17191c" stroke-width="1.6"></circle>'
         '<circle cx="388" cy="402" r="24" fill="none" stroke="#17191c" stroke-width="1" stroke-dasharray="3 3"></circle></svg>')
INSET = (1150, 1650, 1450, 1950)


# The corners of the main map frame of the sample page. The frame is a polygon.
CORNERS = ((190, 210), (1330, 210), (1330, 900), (1480, 985), (1480, 1500), (1330, 1585), (1330, 1840), (190, 1840), (190, 1000), (20, 915), (20, 480), (190, 395))


# A selected polygon frame has a handle at each vertex.
def phandles(pts, r=20):
    return ('<svg class="lay" viewBox="0 0 1500 2000" role="img" aria-label="Frame handles"><path d="M%sZ" fill="none" stroke="#45c4a8" stroke-width="5"></path>'
            % 'L'.join('%d %d' % p for p in pts) + '<g fill="#45c4a8" stroke="#0b1f1a" stroke-width="3">'
            + ''.join('<circle cx="%d" cy="%d" r="%d"></circle>' % (x, y, r) for x, y in pts) + '</g></svg>')


# The selected map frame on a page has a box with eight handles.
def fhandles(box, r=20):
    x1, y1, x2, y2 = box
    mx, my = (x1 + x2) // 2, (y1 + y2) // 2
    pts = [(x, y) for x in (x1, mx, x2) for y in (y1, my, y2) if (x, y) != (mx, my)]
    return ('<svg class="lay" viewBox="0 0 1500 2000" role="img" aria-label="Frame handles"><rect x="%d" y="%d" width="%d" height="%d" fill="none" stroke="#45c4a8" stroke-width="5"></rect>'
            '<g fill="#45c4a8" stroke="#0b1f1a" stroke-width="3">' % (x1, y1, x2 - x1, y2 - y1)
            + ''.join('<circle cx="%d" cy="%d" r="%d"></circle>' % (x, y, r) for x, y in pts) + '</g></svg>')


CIRC = ''.join('<circle cx="%d" cy="%d" r="%s"></circle>' % (x, y, r) for y, r in ((420, 70.6), (820, 70.1), (1220, 70.1), (1620, 70.6)) for x in (420, 760, 1100))
RAMP = (('#a2b291', '0'), ('#b2bf99', '100'), ('#c8ce9f', '200'), ('#e2dca6', '400'), ('#e9d49c', '700'), ('#dcb98b', '1,000'), ('#cc9a7b', '1,400'), ('#bd7f6e', '1,800'))
ROWS = (('Ardent', 'Town', '3', ''), ('Castellane', 'City', '2', 'Port'), ('Hartle', 'Town', '3', ''), ('', 'Town', '3', ''),
        ('Ostrey', 'Town', '3', ''), ('Tolver', 'Town', '3', ''), ('Yarrow', 'Town', '3', 'Port'))


def pg(n, name, sel=False):
    return '<div class="row%s" style="gap: 12px; padding-top: 6px; padding-bottom: 6px"><span class="mono mute">%d</span><div class="thumb">%s</div><span>%s</span></div>' % (
        ' sel' if sel else '', n, FULL % '' if sel else '', name)


# iPad boards. All sizes are in points. Each control is at least 44 points on each side.
# The top 24 points hold the system status bar, and the bottom 20 points hold the home indicator.
BATTERY = ('<svg viewBox="0 0 28 13" style="width: 26px; height: 12px" aria-hidden="true"><rect x=".5" y=".5" width="23" height="12" rx="3.5" fill="none" stroke="currentColor" opacity=".5"></rect>'
           '<rect x="2" y="2" width="20" height="9" rx="2" fill="currentColor"></rect><rect x="25" y="4" width="2" height="5" rx="1" fill="currentColor" opacity=".5"></rect></svg>')


def ipage(name, title, inner, w=1194, h=834, ink=False, split=False):
    html = '''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>%s</title>
<script src="./support.js"></script>
<link rel="stylesheet" href="./vale.css">
</head>
<body>
<x-dc>
<helmet>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono&family=IBM+Plex+Sans:wght@400;500;600&display=swap">
<style>
body{margin:0}
a{color:#45c4a8}
</style>
</helmet>
<div class="app pad" style="height: %dpx">
<div class="sys%s" aria-hidden="true"><span>9:41&ensp;Fri Oct 9</span>%s</div>
%s
<div class="home%s" aria-hidden="true"></div>
</div>
</x-dc>
<script type="text/x-dc" data-dc-script data-props='{"$preview":{"width":%d,"height":%d}}'>
class Component extends DCLogic {
renderVals() {
return {};
}
}
</script>
</body>
</html>
''' % (title, h, ' ink' if ink else '', '' if split else BATTERY, inner, ' ink' if ink else '', w, h)
    open('%s/%s.dc.html' % (OUT, name), 'w').write(html)


def ic(path):
    return '<svg viewBox="0 0 24 24">%s</svg>' % path


def tbtn(name, path, cls=''):
    return '<button class="tool%s" aria-label="%s">%s</button>' % (cls, name, ic(path))


CHEVRON = '<path d="M6 9l6 6 6-6"></path>'
CLOSE = '<path d="M6 6l12 12M18 6L6 18"></path>'
TICK = '<path d="M5 12l5 5 9-10"></path>'
BACK = '<path d="M15 6l-6 6 6 6"></path>'
PLUS = '<path d="M12 5v14M5 12h14"></path>'
NEXT = '<path d="M9 6l6 6-6 6"></path>'
EYE = '<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"></path><circle cx="12" cy="12" r="3"></circle>'
MORE = '<path d="M5 4v16M12 4v16M19 4v16"></path><circle cx="5" cy="9" r="2"></circle><circle cx="12" cy="15" r="2"></circle><circle cx="19" cy="8" r="2"></circle>'
UNDO = '<path d="M8 5L3 10l5 5M3 10h11a5.5 5.5 0 010 11h-3"></path>'
REDO = '<path d="M16 5l5 5-5 5M21 10H10a5.5 5.5 0 000 11h3"></path>'
LAYERS_IC = '<path d="M12 4l9 5-9 5-9-5zM3 14l9 5 9-5"></path>'
TOOLBOX = '<path d="M4 9h16v10H4zM9 9V6h6v3M4 14h16"></path>'

WORKSPACES = (('World', 'PadDraw'), ('Atlas', 'PadAtlas'))
DMODES = (('Globe', 'PadDraw'), ('Flat', 'PadDrawFlat'))


def links(set_, on):
    return ''.join('<a%s href="%s.dc.html">%s</a>' % (' class="on"' if n == on else '', f, n) for n, f in set_)


# The top row has three places: workspace at the left, mode at the center, and actions at the right.
def wswitch(tab):
    return '<nav class="seg card" aria-label="Workspace" style="left: 16px; top: 32px">%s</nav>' % links(WORKSPACES, tab)


def wmenu(tab, on=False):
    return ('<button class="card menu%s" aria-label="Workspace" aria-haspopup="menu" style="left: 16px; top: 32px">%s%s</button>'
            % (' on' if on else '', tab, ic(CHEVRON)))


def modes(set_, on, label):
    return '<nav class="seg card" aria-label="%s" style="left: 50%%; top: 32px; transform: translateX(-50%%)">%s</nav>' % (label, links(set_, on))


def actions(on=None):
    return ('<div class="card bar" role="toolbar" aria-label="Actions" style="right: 16px; top: 32px">' + tbtn('Undo', UNDO) + tbtn('Redo', REDO, ' off')
            + '<span class="div"></span>' + tbtn('Layers', LAYERS_IC, ' on' if on == 'Layers' else '') + tbtn('Toolbox', TOOLBOX, ' on' if on == 'Toolbox' else '') + '</div>')


# The tool strip is on the left edge, below the top row. Each tool has an icon and a short name.
PDTOOLS = tuple((n, n, p) for n, p in DTOOLS[:5]) + (('Pan', 'Move', TOOLS[1][1]),)
PMTOOLS = tuple((n, s, p) for (n, p), s in zip(TOOLS, ('Select', 'Move', 'Point', 'Vertex', 'Label')))
PATOOLS = (PMTOOLS[0], PMTOOLS[1], PMTOOLS[4])
PPOINT = PDTOOLS[:4] + (('Place point', 'Point', TOOLS[2][1]),) + PDTOOLS[5:]


def strip(set_, on):
    return '<div class="card ptools" role="toolbar" aria-label="Tools" style="left: 16px; top: 96px">%s</div>' % ''.join(
        '<button class="tool%s" aria-label="%s">%s<span>%s</span></button>' % (' on' if n == on else '', n, ic(p), s) for n, s, p in set_)


def vslider(label, value, pct):
    return ('<div class="vs" role="slider" aria-label="%s" aria-orientation="vertical" aria-valuenow="%d" aria-valuemin="0" aria-valuemax="100" tabindex="0">'
            '<span class="mono">%s</span><div class="trk"><div style="height: %d%%"><i></i></div></div><span>%s</span></div>' % (label, pct, value, pct, label))


def brushcard(on=False):
    return ('<div class="card ptools" role="group" aria-label="Brush" style="left: 16px; top: 428px">' + vslider('Size', '120 km', 40) + vslider('Flow', '30%', 30)
            + '<button class="tool%s" aria-label="Brush settings">%s<span>Brush</span></button></div>' % (' on' if on else '', ic(MORE)))


def panel(title, body, style, close=True, back=None, act=''):
    head = (tbtn('Back to ' + back, BACK) if back else '') + '<span class="grow">%s</span>' % title + act + (tbtn('Close', CLOSE) if close else '')
    return ('<section class="card panel" aria-label="%s" style="%s"><div class="ph%s">%s</div><div class="pb">%s</div></section>'
            % (title, style, ' bk' if back else '', head, body))


def sheet(title, body, height):
    return ('<section class="card panel sheetp" aria-label="%s" style="height: %dpx"><div class="grab"></div><div class="ph"><span class="grow">%s</span>%s</div><div class="pb">%s</div></section>'
            % (title, height, title, tbtn('Close', CLOSE), body))


KINDS = {'raster': '<path d="M4 4h16v16H4zM4 12h16M12 4v16"></path>', 'polygon': '<path d="M5 7l8-3 6 6-3 9-9-1z"></path>',
         'line': '<path d="M4 18c4-10 8 2 16-12"></path>', 'point': '<circle cx="12" cy="12" r="3"></circle>'}


def lrow(name, kind, sel=False, vis=True, badge=''):
    return ('<div class="row%s"><span class="ty">%s</span><span class="grow">%s</span>%s%s%s</div>' % (
        ' sel' if sel else '', ic(KINDS[kind]), name, '<span class="badge">%s</span>' % badge if badge else '',
        tbtn(('Hide ' if vis else 'Show ') + name, EYE, '' if vis else ' off'), tbtn('Open ' + name, NEXT)))


def more(name):
    return '<div class="row"><span class="grow">%s</span><span class="chev">%s</span></div>' % (name, ic(NEXT))


def pslider(label, value, pct):
    return ('<div class="row"><span class="lab" style="flex-basis: 72px">%s</span><div class="sl" role="slider" aria-label="%s" aria-valuenow="%d" aria-valuemin="0" aria-valuemax="100" tabindex="0">'
            '<div style="width: %d%%"><i></i></div></div><span class="mono mute" style="flex: 0 0 56px; text-align: right">%s</span></div>' % (label, label, pct, pct, value))


def switch(label, on=True):
    return '<div class="row"><span class="grow">%s</span><span class="swt%s" role="switch" aria-label="%s" aria-checked="%s" tabindex="0"></span></div>' % (
        label, ' on' if on else '', label, 'true' if on else 'false')


def globe(style):
    return '<div class="globe" style="position: absolute; max-width: none; %s">%s</div>' % (style, GLOBE)


READ = '<div class="card read mono" style="%s">24.6°E 31.2°N · 640 m</div>'
READ_MID = READ % 'left: 50%; bottom: 28px; transform: translateX(-50%)'
READ_LEFT = READ % 'left: 16px; bottom: 26px'

LAYERS_BODY = (lrow('Height', 'raster', sel=True) + lrow('Precipitation', 'raster', vis=False) + lrow('Topography', 'polygon', badge='Out of date')
               + lrow('Bathymetry', 'polygon') + lrow('Rivers', 'line') + lrow('Borders', 'line') + lrow('Places', 'point')
               + lrow('coast-scan.png', 'raster', badge='Not placed'))
LAYERS_ADD = tbtn('Add a layer', PLUS)
ADD_BODY = (more('Raster') + more('Lines') + more('Points') + more('Polygons') + '<div class="rule"></div>' + more('From a file') + more('From a tool'))

# Each raster layer of values has a unit, a ramp, and band limits. The sea bands come first in SEA.
SEA = (('#86a8b8', '−3,000'), ('#9dbcc6', '−1,000'), ('#b6d0d3', '−200'))
BANDS = tuple(reversed(SEA + RAMP))
HEIGHT_BODY = (field('Unit', 'm', True) + '<h2>Style</h2><div class="seg" role="group" aria-label="Show as"><button class="on">Bands</button><button>Gradient</button><button>Grey</button></div>'
               + '<div class="row"><span class="lab">Ramp</span><span class="grad" style="background: linear-gradient(90deg, %s)"></span></div>' % ', '.join(c for c, _ in SEA + RAMP)
               + '<h2>Band limits</h2><div class="grid2">'
               + ''.join('<div class="row"><span class="sw" style="background: %s"></span><input class="in mono" value="%s" aria-label="Limit %s"></div>' % (c, v, v) for c, v in BANDS)
               + '<div class="row"><button class="btn sec" style="flex: 1">Add a limit</button></div></div>'
               + '<div class="note">The preview and the Polygonize tool use these limits. Each raster layer has its own.</div>')

CURVE = ('<svg class="curve" viewBox="0 0 308 96" role="img" aria-label="Pressure curve"><g stroke="rgba(255,255,255,.1)" stroke-width="1"><path d="M77 0v96M154 0v96M231 0v96M0 48h308"></path></g>'
         '<path d="M8 88L300 8" fill="none" stroke="#45c4a8" stroke-width="2"></path><circle cx="154" cy="48" r="7" fill="#e7e9ec"></circle></svg>')
BRUSH_BODY = (pslider('Size', '120 km', 40) + pslider('Hardness', '20%', 20) + pslider('Flow', '30%', 30) + pslider('Spacing', '10%', 10) + pslider('Smoothing', '40%', 40)
              + '<h2>Pen pressure</h2>' + switch('Pressure sets the size', False) + switch('Pressure sets the flow') + CURVE
              + '<div class="seg" role="group" aria-label="Pressure curve"><button>Soft</button><button class="on">Linear</button><button>Firm</button></div>'
              + switch('Size follows the zoom'))
TOOL_BODY = (pick('Input', 'Height', 'Precipitation') + '<div class="row"><span class="lab">Limits</span><span class="grow">11, from the layer</span></div>' + pslider('Smooth', '60%', 60)
             + '<h2>Output</h2>' + switch('Split into two layers at 0 m') + field('Above', 'Topography') + field('Below', 'Bathymetry')
             + '<div class="note">Topography is out of date. This run replaces it.</div>'
             + '<div class="row"><button class="btn pri" style="flex: 1">Run</button></div>')
MENU_BODY = ('<div class="row sel"><span class="grow">World</span></div>'
             '<div class="row sub2"><span class="grow">Globe</span><span class="chev on">%s</span></div><div class="row sub2"><span class="grow">Flat</span></div>'
             '<div class="rule"></div><div class="row"><span class="grow">Atlas</span></div>' % ic(TICK))

# Wide: 1,000 points or more. Landscape, full screen.
DRAW = wswitch('World') + modes(DMODES, 'Globe', 'View') + strip(PDTOOLS, 'Raise') + READ_MID
WIDE_GLOBE = globe('left: 50%; top: 50%; width: 760px; transform: translate(-50%, -50%)')
ipage('PadDraw', 'Vale draw on iPad', WIDE_GLOBE + DRAW + actions() + brushcard())
ipage('PadDrawOpen', 'Vale draw on iPad with panels', WIDE_GLOBE + DRAW + actions('Layers') + brushcard(True)
      + panel('Brush', BRUSH_BODY, 'left: 92px; top: 96px; width: 340px') + panel('Layers', LAYERS_BODY, 'right: 16px; top: 96px; width: 320px', act=LAYERS_ADD.replace('class="tool"', 'class="tool on"'))
      + panel('Add a layer', ADD_BODY, 'right: 344px; top: 96px; width: 220px', close=False).replace('class="card panel"', 'class="card panel menup"'))
# A layer has one style, and the panel of the layer holds it. The same panel opens in each workspace.
def color(label, hex_):
    return '<div class="row"><span class="lab">%s</span><span class="sw" style="background: #%s"></span><span class="mono mute">%s</span></div>' % (label, hex_, hex_.upper())


def rules(*set_):
    return ('<h2>Style</h2>' + ''.join('<div class="row%s"><span class="grow">%s</span><span class="mono mute">%s</span></div>' % (' sel' if i == 0 else '', n, f)
                                       for i, (n, f) in enumerate(set_)) + '<div class="row"><button class="btn sec" style="flex: 1">Add a rule</button></div>')


RIVER_STYLE = (rules(('Major rivers', 'flow &gt;= 3'), ('Streams', 'flow &lt; 3')) + '<h2>Major rivers</h2>' + field('Filter', 'flow &gt;= 3', True)
               + '<div class="seg" role="group" aria-label="Stroke kind"><button class="on">Solid</button><button>Dashed</button><button>Brush</button></div>'
               + color('Color', '3f8aa3') + field('Width', '0.3 + flow * 0.2', True) + switch('Taper along the line'))
TOPO_STYLE = (rules(('Land', 'elev_min &gt;= 0'), ('Coastline', 'elev_min = 0')) + '<h2>Land</h2>' + field('Filter', 'elev_min &gt;= 0', True)
              + '<div class="seg" role="group" aria-label="Fill kind"><button class="on">Color</button><button>Pattern</button><button>Texture</button></div>'
              + field('Color by', 'elev_min', True) + '<div class="ramp" style="padding-top: 8px">' + ''.join('<span><i style="background: %s"></i>%s</span>' % r for r in RAMP) + '</div>')
SCOPE = '<div class="seg" role="group" aria-label="Style for"><button class="on">All maps</button><button>This map</button></div>'

ipage('PadDrawStyle', 'Vale layer style on the globe on iPad', WIDE_GLOBE + DRAW.replace(strip(PDTOOLS, 'Raise'), strip(PDTOOLS, 'Line')) + actions('Layers') + brushcard()
      + panel('Rivers', RIVER_STYLE, 'right: 16px; top: 96px; width: 320px', back='Layers'))
ipage('PadDrawHeight', 'Vale height layer on iPad', WIDE_GLOBE + DRAW + actions('Layers') + brushcard()
      + panel('Height', HEIGHT_BODY, 'right: 16px; top: 96px; width: 320px', back='Layers'))

# Medium: 600 to 1,000 points. Portrait, full screen. The workspace switch is a menu button.
ipage('PadDrawPortrait', 'Vale draw on iPad in portrait',
      globe('left: 50%; top: 50%; width: 780px; transform: translate(-50%, -50%)') + wmenu('World') + modes(DMODES, 'Globe', 'View') + actions('Toolbox')
      + strip(PDTOOLS, 'Raise') + brushcard() + READ_MID + panel('Polygonize', TOOL_BODY, 'right: 16px; top: 96px; width: 320px', back='Toolbox'), w=834, h=1194)

# Compact: less than 600 points. Split view. The menu button holds the modes, and a panel is a sheet at the bottom.
ipage('PadDrawHalf', 'Vale draw on iPad in a half split view',
      globe('left: -150px; top: 20px; width: 900px') + wmenu('World', True) + actions() + strip(PDTOOLS, 'Raise') + brushcard() + READ_LEFT
      + panel('Workspace', MENU_BODY, 'left: 16px; top: 88px; width: 240px; z-index: 3', close=False).replace('class="card panel"', 'class="card panel menup"'),
      w=597, h=834, split=True)
ipage('PadDrawThird', 'Vale draw on iPad in a narrow split view',
      globe('left: -300px; top: 10px; width: 900px') + wmenu('World') + actions('Layers') + strip(PDTOOLS, 'Raise') + brushcard()
      + sheet('Layers', LAYERS_BODY, 400).replace(tbtn('Close', CLOSE), LAYERS_ADD + tbtn('Close', CLOSE)), w=375, h=834, split=True)

LABEL_MARKS = ('<svg class="lay" viewBox="0 0 1500 2000"><g fill="#0f4a3e" fill-opacity=".1" stroke="#0f4a3e" stroke-width=".8" stroke-dasharray="3 2">'
               '<rect x="956" y="1013" width="77" height="12.5"></rect><rect x="867" y="995" width="77" height="12.5"></rect><rect x="867" y="1013" width="77" height="12.5"></rect></g>'
               '<rect x="954.5" y="993.5" width="80" height="15" fill="none" stroke="#0e8f76" stroke-width="1.6"></rect>'
               '<rect x="890" y="885.5" width="66" height="15" fill="none" stroke="#b36b00" stroke-width="1.2"></rect></svg>')
NOT_PLACED = ('Callow Ferry', 'Little Sable', 'Skerry Rocks')
ZOOM = ('<div class="view bare" style="position: absolute; inset: 0; max-width: none"><div style="position: absolute; left: %dpx; top: %dpx; width: 3000px; height: 4000px">'
        + PLATE % 'Map frame zoomed to Castellane' + LABEL_MARKS + '</div></div>')

# A mode with a list shows the list at the left edge and the detail at the right edge.
SHEET = '<div class="sheet bare" style="position: absolute; left: 50%; top: 96px; width: 528px; max-width: none; transform: translateX(-50%)">'
RIGHT = 'right: 16px; top: 96px'
LEFT = 'left: 16px; top: 96px; width: 280px'

# The Labels tab of a frame edits the label classes of its map. Manual changes to single labels belong to the frame.
def class_list(sel=True):
    return (item('Cities', 'Places', sel) + item('Towns', 'Places') + item('Rivers', 'Rivers and lakes') + item('Regions', 'Province borders')
            + item('Seas', 'Bathymetry'))


CLASS_BODY = (pick('Layer', 'Places') + field('Text', 'name', True) + field('Filter', 'rank &lt;= 2', True) + pick('Font', 'Iowan Old Style')
              + field('Size', '11 pt', True) + color('Color', '#2b2a26') + switch('Halo') + field('Priority', '8', True) + pick('Placement', 'Around the point'))

def pair(label, a, b):
    return ('<div class="row"><span class="lab">%s</span><input class="in mono" value="%s" aria-label="%s, first"><input class="in mono" value="%s" aria-label="%s, second"></div>'
            % (label, a, label, b, label))


# The flat view shows the same world, and the same tools paint on it.
def flat(over, w=1194):
    return ('<div class="view" style="position: absolute; left: 0; top: 50%%; width: %dpx; max-width: none; aspect-ratio: 2 / 1; transform: translateY(-50%%)">' % w
            + '<img class="lay" src="./world-base.svg" alt="Flat map of Kareth with elevation bands"><svg class="lay" viewBox="0 0 1000 500">' + over + '</svg></div>')


FLAT_BRUSH = ('<circle cx="452" cy="262" r="26" fill="none" stroke="#17191c" stroke-width="1"></circle>'
              '<circle cx="452" cy="262" r="14" fill="none" stroke="#17191c" stroke-width=".7" stroke-dasharray="2 2"></circle>')
LINK_RECT = '<rect x="552.8" y="100" width="58.3" height="44.4" fill="#f4f1e6" fill-opacity=".55" stroke="#17191c" stroke-width="1.2" stroke-dasharray="4 2"></rect>'


def link_body(path):
    return ('<div class="row"><span class="lab">File</span><span class="grow mono">%s</span></div><h2>Place on the world</h2>' % path
            + '<div class="sketch">'
            '<img class="lay" src="/_blob/3de283ab580ffe42d27ea4cd6819505d" alt="Scanned coast sketch">'
            '<div class="pt" style="left: 23.8%; top: 25%">1</div><div class="pt" style="left: 81%; top: 75%">2</div></div>'
            '<div class="seg" role="group" aria-label="Method"><button>Globe</button><button>Extents</button><button class="on">Points</button></div>'
            + pair('Point 1', '24°E', '50°N') + pair('Point 2', '36°E', '42°N')
            + '<div class="row"><span class="lab">Extents</span><span class="mono mute">19–40°E · 38–54°N</span></div>'
            '<div class="row"><button class="btn pri" style="flex: 1">Place</button></div>')


SKETCH_PAD = '<div style="position: relative; aspect-ratio: 21 / 16; overflow: hidden; background: #f4f1e6; border-radius: 8px; margin-bottom: 8px">'
FLAT = wswitch('World') + modes(DMODES, 'Flat', 'View') + strip(PDTOOLS, 'Raise') + READ_MID
ipage('PadDrawFlat', 'Vale draw on a flat view on iPad', flat(FLAT_BRUSH) + FLAT + actions() + brushcard())
ipage('PadDrawLinked', 'Vale linked file layer on iPad', flat(LINK_RECT)
      + FLAT + actions('Layers') + brushcard()
      + panel('coast-scan.png', link_body('Sketches/coast-scan.png').replace('<div class="sketch">', SKETCH_PAD), RIGHT + '; width: 320px', back='Layers'))

PROJ_LIST = (item('Lambert conformal conic', '0.5%', True) + item('Transverse Mercator', '0.4%') + item('Albers equal-area conic', '0.9%')
             + item('Azimuthal equal-area', '0.3%') + item('Polar stereographic', '6%') + item('Mercator', '15%', cls='warn') + item('Orthographic')
             + item('Equal Earth') + item('Custom'))
PARAM_BODY = (pair('Center', '30.0°E', '46.0°N') + pair('Parallels', '41.3°N', '50.7°N') + field('Rotation', '0.0°', True) + switch('Fit parallels to the view')
              + '<h2>Distortion</h2><table class="t"><thead><tr><th></th><th>Scale</th><th>Area</th></tr></thead><tbody>'
              '<tr><td>North</td><td class="mono">+0.4%</td><td class="mono">+0.8%</td></tr><tr><td>Center</td><td class="mono">−0.3%</td><td class="mono">−0.6%</td></tr>'
              '<tr><td>South</td><td class="mono">+0.5%</td><td class="mono">+1.0%</td></tr></tbody></table>' + switch('Tissot circles')
              + '<h2>PROJ</h2><textarea class="in mono" rows="3" aria-label="PROJ string" style="width: 100%">+proj=lcc +lat_1=41.3 +lat_2=50.7 +lat_0=46 +lon_0=30 +R=4820000</textarea>')
# The attribute table is in the World workspace, below the flat view. The mark is the new point.
DATA_MARK = ('<circle cx="583" cy="124" r="6" fill="none" stroke="#0e8f76" stroke-width="1.2"></circle>'
             '<circle cx="583" cy="124" r="2.6" fill="#45c4a8" stroke="#0b1f1a" stroke-width=".8"></circle>')
PLACE_BODY = ('<div class="row"><span class="lab">Name</span><input class="in" aria-label="Name"></div>' + pick('Kind', 'Town', 'City', 'Capital') + field('Rank', '3', True)
              + switch('Port', False) + '<div class="row"><span class="lab">Position</span><span class="mono mute">29.27°E 45.70°N</span></div>'
              '<div class="row"><button class="btn pri" style="flex: 1">Add</button></div>')


def places(rows):
    return ('<table class="t"><thead><tr><th>Name</th><th>Kind</th><th>Rank</th><th>Port</th></tr></thead><tbody>'
            + ''.join('<tr%s><td>%s</td><td>%s</td><td class="mono">%s</td><td class="mute">%s</td></tr>' % (
                (' class="sel"', '<span class="mute">New place</span>', k, r, p) if not n else ('', n, k, r, p)) for n, k, r, p in rows)
            + '</tbody></table>')


ipage('PadData', 'Vale attribute table on iPad', flat(DATA_MARK) + wswitch('World') + modes(DMODES, 'Flat', 'View') + actions('Layers') + strip(PPOINT, 'Place point')
      + panel('New place', PLACE_BODY + '<h2>Snap to</h2>' + switch('Vertices') + switch('Lines'), RIGHT)
      + panel('Places', places(ROWS[1:6]), 'left: 96px; right: 332px; bottom: 16px; width: auto',
              act='<span class="mute" style="font-weight: 400; margin-right: 12px">58</span><input class="in" placeholder="Filter" aria-label="Filter" style="flex: 0 1 200px; margin-right: 8px">'))

PAGES_BODY = pg(1, 'Kareth') + pg(2, 'Vesperan', True) + pg(3, 'Dravenhold') + pg(4, 'Marrow Sea') + '<div class="row"><button class="btn sec" style="flex: 1">Add a page</button></div>'


def frames_body(sel):
    return (item('Vesperan', '1:3,500,000', sel == 'Vesperan') + item('Kareth globe', '1:90,000,000', sel == 'Kareth globe')
            + '<div class="row"><button class="btn sec" style="flex: 1">Add a frame</button></div>')


# A map can be in no frame. The Maps tab lists all maps of the atlas.
MAPS_BODY = (item('Vesperan', '1 frame') + item('Kareth globe', '4 frames') + item('Dravenhold', '1 frame') + item('Marrow Sea', '1 frame')
             + item('Vesperan coast', 'No frame', True) + '<div class="row"><button class="btn sec" style="flex: 1">Add a map</button></div>')
MAP_BODY = (field('Name', 'Vesperan coast') + '<div class="row"><span class="lab">Projection</span><span>Transverse Mercator</span></div>'
            + '<h2>Frames</h2><div class="row"><span class="mute">No frame</span></div>'
            + '<div class="row"><button class="btn pri" style="flex: 1">Place on this page</button></div>')


def ptabs(names, on):
    return '<div class="seg" role="tablist" aria-label="Tabs">%s</div>' % ''.join('<button%s>%s</button>' % (' class="on"' if n == on else '', n) for n in names)


# A map frame is an area on a page that shows one map. The tabs of its panel hold the frame and the map that it shows.
FTABS = ('Frame', 'Projection', 'Layers', 'Labels')
FRAME_TAB = (pick('Map', 'Kareth globe') + pick('Shape', 'Rectangle', 'Polygon') + field('Scale', '1:90,000,000', True) + pair('Center', '24.0°E', '30.0°N')
             + field('Rotation', '0.0°', True) + switch('Border', False) + switch('Graticule'))
PROJ_TAB = pick('Projection', 'Lambert conformal conic') + PARAM_BODY


def map_layers(row):
    return ''.join(row(n, k, vis=n != 'Sea routes') for n, k in (
        ('Bathymetry', 'polygon'), ('Topography', 'polygon'), ('Rivers and lakes', 'line'), ('National border', 'line'),
        ('Province borders', 'line'), ('Roads', 'line'), ('Sea routes', 'line'), ('Places', 'point')))


def pad_page(over=''):
    return (SHEET.replace('sheet bare', 'sheet').replace('left: 50%', 'left: 326px').replace('; transform: translateX(-50%)', '')
            + FULL % 'Atlas page for Vesperan' + over + '</div>')


def atlas_left(tab, sel='Vesperan'):
    body = PAGES_BODY + '<h2>Frames on page 2</h2>' + frames_body(sel) if tab == 'Pages' else MAPS_BODY
    return panel('Atlas', ptabs(('Pages', 'Maps'), tab) + body, 'left: 96px; top: 96px; width: 224px', close=False)


def frame_right(name, tab, body):
    return panel('Frame · ' + name, ptabs(FTABS, tab) + body, RIGHT + '; width: 320px', close=False)


ATLAS_ACT = actions().replace(tbtn('Layers', LAYERS_IC) + tbtn('Toolbox', TOOLBOX), '<button class="btn pri">Export</button>')
PAGE_READ = READ.replace('24.6°E 31.2°N · 640 m', '12 × 16 in · 2 / 4') % 'left: 50%; bottom: 28px; transform: translateX(-50%)'
ATLAS = wswitch('Atlas') + ATLAS_ACT
DISTORT = '<svg class="lay" viewBox="0 0 1500 2000"><g fill="#c2382b" fill-opacity=".14" stroke="#9c2415" stroke-width="3">' + CIRC + '</g></svg>'
ipage('PadAtlas', 'Vale atlas on iPad', ATLAS + pad_page(fhandles(INSET)) + strip(PATOOLS, 'Select') + atlas_left('Pages', 'Kareth globe')
      + frame_right('Kareth globe', 'Frame', FRAME_TAB) + PAGE_READ)
ipage('PadMap', 'Vale maps of the atlas on iPad', ATLAS + pad_page() + strip(PATOOLS, 'Select') + atlas_left('Maps')
      + panel('Map', MAP_BODY, RIGHT + '; width: 320px', close=False) + PAGE_READ)
ipage('PadProjection', 'Vale projection of a map on iPad', ATLAS + pad_page(DISTORT + phandles(CORNERS)) + strip(PATOOLS, 'Select') + atlas_left('Pages')
      + frame_right('Vesperan', 'Projection', PROJ_TAB) + PAGE_READ)
ipage('PadStyle', 'Vale layers of a map on iPad', ATLAS + pad_page(phandles(CORNERS)) + strip(PATOOLS, 'Select') + atlas_left('Pages')
      + frame_right('Vesperan', 'Layers', map_layers(lrow) + '<h2>Style for this map</h2>' + item('Topography', 'Opacity 90%')) + PAGE_READ)
ipage('PadLabels', 'Vale label class of a map on iPad', ATLAS + pad_page(phandles(CORNERS)) + strip(PATOOLS, 'Select') + atlas_left('Pages')
      + panel('Cities', CLASS_BODY, RIGHT + '; width: 320px', close=False, back='Labels') + PAGE_READ)

# You pin, move, and hide a label in a map frame, because each manual change belongs to one frame.
ipage('PadAtlasLabels', 'Vale label editing in a map frame on iPad', ZOOM % (-1300, -1603) + ATLAS + strip(PATOOLS, 'Edit labels')
      + '<div class="card read" style="left: 50%; top: 32px; transform: translateX(-50%)">Page 2 · Frame Vesperan</div>'
      + '<div role="toolbar" aria-label="Label actions" class="card bar" style="left: 590px; top: 440px"><button class="btn">Pin</button><button class="btn">Next</button><button class="btn">Hide</button></div>'
      + panel('Not placed', ''.join(item(n) for n in NOT_PLACED), 'right: 16px; top: 96px; width: 240px'), ink=True)


# Desktop boards. All sizes are in pixels, for a Mac screen of 1440 by 900.
# The window uses the macOS materials: the bars, the rail, and the docks are translucent over the desktop, and the canvas has a darker tint.
# The globe, the flat map, and the pages are opaque.
def dpage(name, title, inner, menus=None):
    html = '''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>%s</title>
<script src="./support.js"></script>
<link rel="stylesheet" href="./vale.css">
</head>
<body>
<x-dc>
<helmet>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono&family=IBM+Plex+Sans:wght@400;500;600&display=swap">
<style>
body{margin:0}
a{color:#45c4a8}
</style>
</helmet>
<div class="app desk">
%s
<div class="win">
%s
</div>
</div>
</x-dc>
<script type="text/x-dc" data-dc-script data-props='{"$preview":{"width":1440,"height":900}}'>
class Component extends DCLogic {
renderVals() {
return {};
}
}
</script>
</body>
</html>
''' % (title, sysbar(menus or MENUS), inner)
    open('%s/%s.dc.html' % (OUT, name), 'w').write(html)


MENUS = ('File', 'Edit', 'View', 'Layer', 'Map', 'Window', 'Help')
APPLE = ('<svg viewBox="0 0 24 24" class="apple"><path d="M15.5 3c.2 1.4-.4 2.7-1.2 3.6-.8.9-2 1.5-3.1 1.4-.2-1.3.4-2.7 1.2-3.5.8-.9 2.1-1.5 3.1-1.5zM19.6 17'
         'c-.5 1.2-.8 1.7-1.5 2.7-1 1.4-2.3 3.2-4 3.2-1.5 0-1.9-1-3.9-1s-2.5 1-4 1c-1.7 0-2.9-1.6-3.9-3C-.2 16.1-.5 11.4 1.2 8.9c1.2-1.8 3-2.8 4.8-2.8 1.8 0 2.9 1 4.4 1 1.4 0 2.3-1 4.4-1 '
         '1.6 0 3.2.9 4.4 2.3-3.8 2.1-3.2 7.6.4 8.6z"></path></svg>')


# The menus are in the menu bar of the system.
def sysbar(menus):
    return ('<div class="sysbar" aria-label="Menu bar">' + APPLE + '<b>Vale</b>' + ''.join('<span>%s</span>' % m for m in menus)
            + '<span class="grow"></span><span>Fri Oct 16&ensp;9:41 AM</span></div>')


KEYS = {'Raise': 'R', 'Lower': 'F', 'Smooth': 'S', 'Flatten': 'E', 'Line': 'L', 'Hand': 'H', 'Select': 'V', 'Place point': 'P', 'Edit vertices': 'A',
        'Edit labels': 'T', 'Undo': '⌘Z', 'Redo': '⇧⌘Z', 'Layers': '⌥⌘1', 'Toolbox': '⌥⌘2', 'Add a layer': '⇧⌘N', 'Brush…': 'B',
        'Previous': '[', 'Next': ']', 'Pin': '↩', 'Hide': '⌫', 'Export…': '⇧⌘E'}
DOTS = '<circle cx="5" cy="12" r="1"></circle><circle cx="12" cy="12" r="1"></circle><circle cx="19" cy="12" r="1"></circle>'
SEARCH = '<circle cx="11" cy="11" r="6"></circle><path d="M16 16l4 4"></path>'
FOLDER = '<path d="M3 7h6l2 2h10v10H3z"></path>'
CLOUD = '<path d="M7 18a4 4 0 010-8 5 5 0 019.6-1.5A4 4 0 0117 18z"></path>'
HAND = ('<path d="M8 13V6a1.5 1.5 0 013 0v5M11 11V4.5a1.5 1.5 0 013 0V11M14 11V6a1.5 1.5 0 013 0v7c0 4-2.5 7-6 7-2.5 0-4-1.5-5.5-4L4 13'
        'a1.5 1.5 0 012.5-1.5L8 13"></path>')
EYE_OFF = '<path d="M3 3l18 18M10.6 6.1A10 10 0 0112 6c6 0 10 6 10 6a17 17 0 01-3.2 3.8M6.6 6.6C3.8 8.4 2 12 2 12s4 6 10 6a9.6 9.6 0 004.4-1"></path>'
GRID = '<path d="M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z"></path>'
LIST = '<path d="M4 6h16M4 12h16M4 18h16"></path>'
SIDEBAR = '<path d="M3 5h18v14H3zM9 5v14"></path>'

WTOOLS = DTOOLS[:5] + (TOOLS[2], TOOLS[0], TOOLS[3], ('Hand', HAND))
ATOOLS = (TOOLS[0], ('Hand', HAND), TOOLS[4])


def dbtn(name, path, cls=''):
    key = KEYS.get(name, '')
    return '<button class="tool%s" aria-label="%s" title="%s">%s</button>' % (cls, name, name + (' (%s)' % key if key else ''), ic(path))


def tbtn2(label, cls='btn'):
    key = KEYS.get(label, '')
    return '<button class="%s" title="%s">%s</button>' % (cls, label + (' (%s)' % key if key else ''), label)


# The title bar and the tool bar are one bar. It has three places: the window and the workspace at the left, the view or the mode at the center,
# and the actions at the right. The actions are the same in each workspace.
def tbar(left, center=''):
    return ('<div class="tbar"><div class="tl"><span class="lights" aria-hidden="true"><i></i><i></i><i></i></span>%s</div><div class="tc">%s</div>'
            '<div class="tr">%s</div></div>' % (left, center, ACTIONS))


ACTIONS = (dbtn('Undo', UNDO) + dbtn('Redo', REDO, ' off') + '<span class="div"></span>' + dbtn('Layers', SIDEBAR, ' on'))
DOC = '<div class="doc"><b>Kareth</b><span>kareth.gpkg · Saved<span class="mark" aria-label="Saved">%s</span></span></div>' % ic(TICK)


def head(ws, center=''):
    return tbar(DOC + dseg(DWS, ws, 'Workspace'), center)


DWS = (('World', 'Draw'), ('Atlas', 'Atlas'))
DVIEW = (('Globe', 'Draw'), ('Flat', 'DrawFlat'))


def dseg(set_, on, label):
    return '<nav class="seg" aria-label="%s">%s</nav>' % (label, links(set_, on))


def popup(text, label):
    return '<button class="menu" aria-label="%s" aria-haspopup="menu">%s%s</button>' % (label, text, ic(CHEVRON))




# The rail holds the tools. A tooltip shows the name and the key of each tool.
def drail(set_, on, hov=None, more=False):
    tools = ''.join(dbtn(n, p, (' on' if n == on else '') + (' hov' if n == hov else '')) for n, p in set_)
    return '<div class="drail" role="toolbar" aria-label="Tools">%s%s</div>' % (tools, dbtn('More tools', DOTS) if more else '')


def tip(name, style):
    key = KEYS.get(name, '')
    return '<div class="tip" role="tooltip" style="%s">%s%s</div>' % (style, name, '<span class="kbd">%s</span>' % key if key else '')


# The options bar holds the settings of the active tool. Each mode has one, so the canvas does not move when the mode changes.
def obar(*parts):
    return '<div class="obar" role="toolbar" aria-label="Tool options">%s</div>' % ''.join(parts)


def osl(label, value, pct):
    return ('<span class="ol">%s</span><div class="sl" role="slider" aria-label="%s" aria-valuenow="%d" aria-valuemin="0" aria-valuemax="100" tabindex="0">'
            '<div style="width: %d%%"><i></i></div></div><span class="mono ov">%s</span>' % (label, label, pct, pct, value))


def osw(label, on=True):
    return '<span class="ow"><span class="swt%s" role="switch" aria-label="%s" aria-checked="%s" tabindex="0"></span>%s</span>' % (
        ' on' if on else '', label, 'true' if on else 'false', label)


def osel(label, value, width=140):
    return '<span class="ol">%s</span><select class="in" aria-label="%s" style="width: %dpx"><option>%s</option></select>' % (label, label, width, value)


OSEP = '<span class="div"></span>'
GROW = '<span class="grow"></span>'
BRUSH_OPTS = obar('<span class="on">Raise</span>', OSEP, osl('Size', '120 km', 40), osl('Flow', '30%', 30), osl('Hardness', '20%', 20), OSEP,
                  osw('Pressure sets the flow'), GROW, tbtn2('Brush…'))
LINE_OPTS = obar('<span class="on">Line</span>', OSEP, osel('Layer', 'Rivers'), osl('Smoothing', '40%', 40), OSEP, osw('Snap to lines'), osw('Close the line', False))
CLASS_OPTS = obar('<span class="on">Select</span>', OSEP, tbtn2('Add a class'), OSEP, osw('Show overlaps', False))
LABEL_OPTS = obar('<span class="on">Label</span>', OSEP, tbtn2('Previous') + tbtn2('Next') + tbtn2('Pin') + tbtn2('Hide'), OSEP, osw('Show candidates'), osw('Show overlaps'))
DATA_OPTS = obar('<span class="on">Point</span>', OSEP, osel('Layer', 'Places'), OSEP, osw('Snap to vertices'), osw('Snap to lines'))
PAGE_OPTS = obar('<span class="on">Select</span>', OSEP, osw('Show the margins'), osw('Show the bleed', False))


# A dock is a column at the right edge. Its tabs keep the same order in a workspace. Below the tabs is the list, and below the list is the inspector,
# which is the panel of the selection. With no selection, the inspector holds the world.
def dock(tabs, on, body='', insp=None, act='', list_h=None):
    head_ = ('<div class="dh">' + ''.join('<span class="dt%s">%s</span>' % (' on' if t == on else '', t) for t in tabs) + GROW + act + '</div>')
    lst = '<div class="dl"%s>%s</div>' % (' style="flex: 0 0 %dpx"' % list_h if list_h else '', body) if body else ''
    ins = ('<section class="insp" aria-label="%s"><div class="ih">%s</div><div class="ib">%s</div></section>' % (insp[0], insp[0], insp[1])) if insp else ''
    return '<aside class="dock">%s%s%s</aside>' % (head_, lst, ins)


# The eye of a layer always shows, so a hidden layer is easy to see.
def drow(name, kind, sel=False, vis=True, badge='', hov=False):
    return ('<div class="row%s%s"><span class="ty">%s</span><span class="grow">%s</span>%s%s</div>' % (
        ' sel' if sel else '', ' hov' if hov else '', ic(KINDS[kind]), name, '<span class="badge">%s</span>' % badge if badge else '',
        dbtn(('Hide ' if vis else 'Show ') + name, EYE if vis else EYE_OFF, '' if vis else ' off')))


def wlayers(sel=None, hov=None):
    return ''.join(drow(n, k, n == sel, v, b, n == hov) for n, k, v, b in (
        ('Height', 'raster', True, ''), ('Precipitation', 'raster', False, ''), ('Topography', 'polygon', True, 'Out of date'),
        ('Bathymetry', 'polygon', True, ''), ('Rivers', 'line', True, ''), ('Borders', 'line', True, ''), ('Places', 'point', True, ''),
        ('coast-scan.png', 'raster', True, 'Not placed')))


def mlayers(sel=None):
    return ''.join(drow(n, k, n == sel, n != 'Sea routes') for n, k in (
        ('Bathymetry', 'polygon'), ('Topography', 'polygon'), ('Rivers and lakes', 'line'), ('National border', 'line'),
        ('Province borders', 'line'), ('Roads', 'line'), ('Sea routes', 'line'), ('Peaks', 'point'), ('Places', 'point')))


ADD = dbtn('Add a layer', PLUS)
WTABS = ('Layers', 'Toolbox')


def canvas(inner, cls=''):
    return '<main class="cv%s">%s</main>' % (cls, inner)


def main(*cols):
    return '<div class="main">%s</div>' % ''.join(cols)


def sbar(*parts):
    return '<footer class="sbar">%s</footer>' % ''.join(GROW if p is None else '<span>%s</span>' % p for p in parts)


# A popup menu. A row can have a key, a submenu, a tick, or the hover state.
def pop(rows, style, label='Menu'):
    return '<div class="pop" role="menu" aria-label="%s" style="%s">%s</div>' % (label, style, ''.join(rows))


def mi(label, key='', sub=False, on=False, off=False, hov=False):
    cls = ''.join(c for c, f in ((' on', on), (' off', off), (' hov', hov)) if f)
    return '<div class="mi%s" role="menuitem"><span class="tk">%s</span><span class="grow">%s</span><span class="kbd">%s</span>%s</div>' % (
        cls, ic(TICK) if on else '', label, key, '<span class="chev">%s</span>' % ic(NEXT) if sub else '')


MR = '<div class="mr"></div>'

WORLD_STATUS = sbar('24.6°E 31.2°N · 640 m', None, 'Face 4,096', '100%')
DGLOBE = globe('left: 50%; top: 50%; width: 700px; transform: translate(-50%, -50%)')
DSHEET = '<div class="sheet bare" style="position: absolute; left: 50%%; top: %dpx; width: %dpx; max-width: none; transform: translateX(-50%%)">'
GLOBE_VIEW = dseg(DVIEW, 'Globe', 'View')
FLAT_VIEW = dseg(DVIEW, 'Flat', 'View') + popup('Equirectangular', 'Projection') + tbtn2('Recenter') + tbtn2('Reset')


def world(center, rail, opts, cv, right, status=WORLD_STATUS):
    return head('World', center) + opts + main(rail, canvas(cv), right) + status


WRAIL = drail(WTOOLS, 'Raise', more=True)
WORLD_BODY = (field('Name', 'Kareth') + field('Radius', '4,820 km', True) + '<div class="row"><span class="lab">1°</span><span class="mono mute">84.1 km</span></div>'
              + '<div class="row"><span class="lab">Face size</span><span class="mono mute">4,096</span></div>'
              + '<h2>Heightmap</h2><div class="row"><button class="btn sec" style="flex: 1">Import…</button><button class="btn sec" style="flex: 1">Export GeoTIFF…</button></div>'
              + '<div class="note">Select a layer or a tool to see its settings here.</div>')
HEIGHT_IO = ('<div class="row"><button class="btn sec" style="flex: 1">Import heightmap…</button><button class="btn sec" style="flex: 1">Export GeoTIFF…</button></div>'
             + HEIGHT_BODY)
LINE_GLOBE = globe('left: 50%; top: 50%; width: 700px; transform: translate(-50%, -50%)').replace(
    GLOBE[GLOBE.index('<svg'):],
    '<svg class="lay" viewBox="0 0 1000 1000"><path d="M300 300C330 340 350 360 370 385S384 398 388 402" fill="none" stroke="#3f8aa3" stroke-width="3" stroke-linecap="round"></path>'
    '<g fill="#fff" stroke="#17191c" stroke-width="1.2"><circle cx="300" cy="300" r="3.5"></circle><circle cx="340" cy="350" r="3.5"></circle><circle cx="370" cy="385" r="3.5"></circle></g>'
    '<g stroke="#17191c" stroke-width="1.4"><path d="M388 390v8M388 406v8M376 402h8M392 402h8"></path></g></svg>')

# Start screen. The sidebar lists the places. The projects are a grid of cards, and the actions of a card are in its menu.
def pcard(name, date, where, hue=0, pic=True, badge='', hov=False):
    img = ('<img src="./globe.svg" alt="" style="filter: hue-rotate(%ddeg)">' % hue) if pic else '<span class="dl2">%sNot downloaded</span>' % ic(CLOUD)
    return ('<article class="pcard%s"><div class="pic">%s%s%s</div><div class="pm"><div class="pn">%s</div>'
            '<div class="mute">%s</div><div class="mute pw">%s%s</div></div></article>'
            % (' hov' if hov else '', img, '<span class="badge">%s</span>' % badge if badge else '',
               '<button class="tool more" aria-label="Actions" aria-haspopup="menu">%s</button>' % ic(DOTS),
               name if pic else name.lower().replace(' ', '-') + '.gpkg', date, ic(CLOUD if where == 'iCloud Drive' else FOLDER), where))


def side(*rows):
    return '<aside class="side2">%s</aside>' % ''.join(rows)


def srow(name, path, sel=False):
    return '<div class="row%s">%s<span>%s</span></div>' % (' sel' if sel else '', ic(path), name)


dpage('Start', 'Vale start screen',
      tbar('<div class="doc"><b>Worlds</b><span>6 worlds</span></div>',
           '')
      .replace(ACTIONS, '<div class="search">%s<input class="in" placeholder="Search" aria-label="Search worlds"></div>' % ic(SEARCH)
               + popup('Last changed', 'Sort') + '<nav class="seg" aria-label="Show as"><a class="on" title="Grid">%s</a><a title="List">%s</a></nav>' % (ic(GRID), ic(LIST))
               + '<button class="btn pri" title="New world (⌘N)">New world…</button>')
      + main(side('<div class="gh">Worlds</div>', srow('All worlds', GRID, True), srow('On this Mac', FOLDER), srow('iCloud Drive', CLOUD),
                  '<div class="gh">Recent</div>', srow('Kareth', KINDS['polygon']), srow('Isles of Morrow', KINDS['polygon']), srow('The Sundering', KINDS['polygon'])),
             canvas('<div class="start">'
                    + pcard('Kareth', 'Today, 9:12 AM', 'On this Mac') + pcard('Isles of Morrow', 'Oct 12, 2026', 'iCloud Drive', 140, badge='2 versions', hov=True)
                    + pcard('The Sundering', 'Oct 3, 2026', 'On this Mac', 260) + pcard('Ashfall Reach', 'Sep 28, 2026', 'iCloud Drive', 40)
                    + pcard('Tidewater', 'Sep 9, 2026', 'On this Mac', 200) + pcard('Low Countries', 'Aug 30, 2026', 'iCloud Drive', pic=False)
                    + '</div>', ' start-cv'))
      + pop([mi('Open', '⌘O'), MR, mi('Rename…'), mi('Make a copy', '⌘D'), mi('Show in Finder', '⌥⌘R'), MR, mi('Delete…', '⌘⌫', hov=True)],
            'left: 750px; top: 150px; width: 220px', 'Actions'),
      menus=('File', 'Edit', 'View', 'Window', 'Help'))

# World workspace
dpage('Draw', 'Vale draw', world(GLOBE_VIEW, drail(WTOOLS, 'Raise', hov='Smooth', more=True), BRUSH_OPTS, DGLOBE,
                                 dock(WTABS, 'Layers', wlayers(hov='Rivers'), ('Kareth', WORLD_BODY), ADD, list_h=232))
      + tip('Smooth', 'left: 48px; top: 210px'))
dpage('DrawOpen', 'Vale draw with the brush settings', world(GLOBE_VIEW, WRAIL, BRUSH_OPTS, DGLOBE,
                                                              dock(WTABS, 'Layers', wlayers('Height'), ('Height', HEIGHT_IO), ADD, list_h=232)).replace('title="Brush… (B)">', 'title="Brush… (B)" aria-expanded="true" style="background: var(--sel); color: #c9f5ea">')
      + '<section class="pop popover" aria-label="Brush" style="right: 8px; top: 104px; width: 320px"><div class="ih">Brush</div><div class="ib">%s</div></section>' % BRUSH_BODY)
dpage('DrawHeight', 'Vale height layer', world(GLOBE_VIEW, WRAIL, BRUSH_OPTS, DGLOBE, dock(WTABS, 'Layers', wlayers('Height'), ('Height', HEIGHT_IO), ADD, list_h=232)))
dpage('DrawFlat', 'Vale draw on a flat view', world(FLAT_VIEW, WRAIL, BRUSH_OPTS, flat(FLAT_BRUSH, 1096),
                                                    dock(WTABS, 'Layers', wlayers(), ('Kareth', WORLD_BODY), ADD.replace('class="tool"', 'class="tool on"'), list_h=232))
      + pop([mi('Raster'), mi('Lines', '⇧⌘N', hov=True), mi('Points'), mi('Polygons'), MR, mi('From a file…', '⇧⌘O'), mi('From a tool', sub=True)],
            'right: 14px; top: 144px; width: 236px', 'Add a layer'))
dpage('DrawLinked', 'Vale linked file layer', world(FLAT_VIEW, WRAIL, BRUSH_OPTS, flat(LINK_RECT, 1096),
                                                    dock(WTABS, 'Layers', wlayers('coast-scan.png'), ('coast-scan.png', link_body('~/Sketches/coast-scan.png')
                                                         .replace('</span></div><h2>', '</span></div><div class="row"><a href="#" style="margin-left: 76px">Show in Finder</a></div><h2>', 1)), ADD, list_h=232),
                                                    status=sbar('31.8°E 47.4°N', None, 'Face 4,096', '100%')))
dpage('DrawStyle', 'Vale layer style on the globe', world(GLOBE_VIEW, drail(WTOOLS, 'Line', more=True), LINE_OPTS, LINE_GLOBE,
                                                         dock(WTABS, 'Layers', wlayers('Rivers'), ('Rivers', RIVER_STYLE), ADD, list_h=232)))
dpage('DrawToolbox', 'Vale toolbox', world(GLOBE_VIEW, WRAIL, BRUSH_OPTS, DGLOBE,
                                           dock(WTABS, 'Toolbox', '<div class="row search">%s<input class="in" placeholder="Search" aria-label="Search tools"></div>' % ic(SEARCH)
                                                + item('Polygonize', sel=True) + ''.join(item(t) for t in ('Contour lines', 'Smooth', 'Simplify', 'Dissolve', 'Clip')),
                                                ('Polygonize', TOOL_BODY), list_h=232)))


CANDIDATES = ('<table class="t"><thead><tr><th>Position</th><th>Cost</th></tr></thead><tbody>'
              '<tr class="sel"><td>Northeast</td><td class="mono">0.22</td></tr><tr><td>Southeast</td><td class="mono mute">0.47</td></tr>'
              '<tr><td>Northwest</td><td class="mono mute">0.66</td></tr><tr><td>Southwest</td><td class="mono mute">0.81</td></tr></tbody></table>'
              + '<h2>Class</h2>' + item('Cities', 'rank &lt;= 2'))
LABEL_LIST = ('<div class="gh">Not placed</div>' + ''.join(item(n, r, cls='warn') for n, r in zip(NOT_PLACED, ('Overlap', 'Too long', 'Overlap')))
              + '<div class="gh">Changed by hand</div>' + item('Castellane', 'Selected', True) + item('Deepwell', 'Pinned') + item('Tarn', 'Moved') + item('Sable Ford', 'Hidden'))
DPLACES = ('<table class="t places"><colgroup><col style="width: 38%"><col style="width: 22%"><col style="width: 14%"><col></colgroup>'
           '<thead><tr><th>Name<span class="sort">▲</span></th><th>Kind</th><th>Rank</th><th>Port</th></tr></thead><tbody>'
           + ''.join('<tr%s><td>%s</td><td>%s</td><td class="mono">%s</td><td>%s</td></tr>' % (
               (' class="sel"', '<span class="mute">New place</span>', k, r, '') if not n else ('', n, k, r, '<span class="ok">%s</span>' % ic(TICK) if p else ''))
               for n, k, r, p in ROWS + (('Zell', 'Town', '3', ''),))
           + '</tbody></table>')
DPLACE = PLACE_BODY.replace('aria-label="Name">', 'aria-label="Name" placeholder="Name of the place">', 1).replace('class="in"', 'class="in focus"', 1)
dpage('Data', 'Vale attribute table', head('World', FLAT_VIEW) + DATA_OPTS
      + main(drail(WTOOLS, 'Place point', more=True),
             '<div class="col">' + canvas(flat(DATA_MARK, 1096))
             + '<section class="dock b" aria-label="Places"><div class="dh"><span class="dt on">Places</span><span class="mute">58</span>' + GROW
             + '<input class="in" placeholder="Filter" aria-label="Filter" style="width: 220px"></div><div class="dl">' + DPLACES + '</div></section></div>',
             dock(WTABS, 'Layers', wlayers('Places'), ('New place', DPLACE), ADD, list_h=232))
      + sbar('29.27°E 45.70°N', None, 'Face 4,096', '100%'))

# Atlas workspace
ATLAS_HEAD = head('Atlas', '<button class="tool" aria-label="Previous page">%s</button><span class="pgn">Page 2 of 4</span><button class="tool" aria-label="Next page">%s</button>'
                  % (ic(BACK), ic(NEXT)) + '<span class="div"></span>' + tbtn2('Export…', 'btn pri'))
# The Layers, Projection, and Labels tabs belong to the map of the selected frame.
ATABS = ('Pages', 'Maps', 'Layers', 'Projection', 'Labels')
PAGES_LIST = PAGES_BODY + '<div class="gh">Frames on page 2</div>'
LABELS_LIST = '<div class="gh">Classes</div>' + class_list(False) + LABEL_LIST


def atlas(name, title, opts, over, tabs_on, lst, insp, rail_on='Select', list_h=452, status=sbar('12 × 16 in', None, '62%')):
    page = DSHEET.replace('sheet bare', 'sheet') % (24, 540) + FULL % 'Atlas page for Vesperan' + over + '</div>'
    dpage(name, title, ATLAS_HEAD + opts + main(drail(ATOOLS, rail_on), canvas(page), dock(ATABS, tabs_on, lst, insp, list_h=list_h)) + status)


atlas('Atlas', 'Vale atlas', PAGE_OPTS, fhandles(INSET), 'Pages', PAGES_LIST + frames_body('Kareth globe'), ('Frame · Kareth globe', FRAME_TAB))
atlas('Map', 'Vale maps of the atlas', PAGE_OPTS, '', 'Maps', MAPS_BODY, ('Map · Vesperan coast', MAP_BODY), list_h=232)
atlas('Projection', 'Vale projection of a map', PAGE_OPTS, DISTORT + phandles(CORNERS), 'Projection', PROJ_LIST, ('Vesperan · Lambert conformal conic', PARAM_BODY), list_h=246)
atlas('Style', 'Vale layer style in a map', PAGE_OPTS, phandles(CORNERS), 'Layers', mlayers('Topography'), ('Topography · Vesperan', SCOPE + TOPO_STYLE), list_h=272)
atlas('Labels', 'Vale label classes of a map', CLASS_OPTS, phandles(CORNERS), 'Labels',
      LABELS_LIST.replace(item('Castellane', 'Selected', True), item('Castellane', 'Moved'))
      .replace(item('Cities', 'Places'), item('Cities', 'Places', True), 1), ('Cities', CLASS_BODY), list_h=320)
dpage('AtlasLabels', 'Vale label editing in a map frame', ATLAS_HEAD
      + LABEL_OPTS + main(drail(ATOOLS, 'Edit labels'), canvas(ZOOM % (-1362, -1614)),
                          dock(ATABS, 'Labels', LABELS_LIST, ('Castellane', CANDIDATES), list_h=320))
      + sbar('31.42°E 46.31°N', None, 'Frame Vesperan · 1:3,500,000', '390%'))
