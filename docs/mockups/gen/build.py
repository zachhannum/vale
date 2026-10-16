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
CORNERS = ((190, 210), (1330, 210), (1330, 900), (1480, 985), (1480, 1500), (1330, 1585), (1330, 1840), (190, 1840), (190, 1000), (20, 915), (20, 480), (190, 395))
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

WORKSPACES = (('World', 'PadDraw'), ('Maps', 'PadMap'), ('Atlas', 'PadAtlas'))
DMODES = (('Globe', 'PadDraw'), ('Flat', 'PadDrawFlat'))
MMODES = (('Layout', 'PadMap'), ('Projection', 'PadProjection'), ('Labels', 'PadLabels'), ('Data', 'PadData'))


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
             '<div class="rule"></div><div class="row"><span class="grow">Maps</span></div><div class="row"><span class="grow">Atlas</span></div>' % ic(TICK))

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

FRAME_BODY = (field('Name', 'Vesperan') + '<div class="row"><span class="lab">Projection</span><span>Conformal conic</span></div>' + field('Scale', '1:3,500,000', True)
              + pick('Page', '12 × 16 in') + pick('Style', 'Tinted relief'))
FRAME = '<button class="card menu" aria-label="Map frame" aria-haspopup="menu" style="right: 241px; top: 32px">Vesperan%s</button>' % ic(CHEVRON)

ipage('PadMap', 'Vale map on iPad', wswitch('Maps') + modes(MMODES, 'Layout', 'Map mode') + FRAME + actions()
      + '<div class="sheet bare" style="position: absolute; left: 50%; top: 96px; width: 528px; max-width: none; transform: translateX(-50%)">' + PLATE % 'Map of Vesperan'
      + '<svg class="lay" viewBox="0 0 1500 2000" role="img" aria-label="Outline handles"><g fill="#45c4a8" stroke="#0b1f1a" stroke-width="3">'
      + ''.join('<circle cx="%d" cy="%d" r="16"></circle>' % c for c in CORNERS) + '</g></svg></div>'
      + strip(PMTOOLS, 'Select')
      + panel('Frame', FRAME_BODY, 'right: 16px; top: 96px'))

LABEL_MARKS = ('<svg class="lay" viewBox="0 0 1500 2000"><g fill="#0f4a3e" fill-opacity=".1" stroke="#0f4a3e" stroke-width=".8" stroke-dasharray="3 2">'
               '<rect x="956" y="1013" width="77" height="12.5"></rect><rect x="867" y="995" width="77" height="12.5"></rect><rect x="867" y="1013" width="77" height="12.5"></rect></g>'
               '<rect x="954.5" y="993.5" width="80" height="15" fill="none" stroke="#0e8f76" stroke-width="1.6"></rect>'
               '<rect x="890" y="885.5" width="66" height="15" fill="none" stroke="#b36b00" stroke-width="1.2"></rect></svg>')
NOT_PLACED = ('Callow Ferry', 'Little Sable', 'Skerry Rocks')
ipage('PadLabels', 'Vale labels on iPad',
      '<div class="view bare" style="position: absolute; inset: 0; max-width: none"><div style="position: absolute; left: -1300px; top: -1603px; width: 3000px; height: 4000px">'
      + PLATE % 'Map zoomed to Castellane' + LABEL_MARKS + '</div></div>'
      + wswitch('Maps') + modes(MMODES, 'Labels', 'Map mode') + FRAME + actions() + strip(PMTOOLS, 'Edit labels')
      + '<div role="toolbar" aria-label="Label actions" class="card bar" style="left: 590px; top: 440px"><button class="btn">Pin</button><button class="btn">Next</button><button class="btn">Hide</button></div>'
      + panel('Not placed', ''.join(item(n) for n in NOT_PLACED), 'right: 16px; top: 96px; width: 240px'), ink=True)


# A mode with a list shows the list at the left edge and the detail at the right edge.
SHEET = '<div class="sheet bare" style="position: absolute; left: 50%; top: 96px; width: 528px; max-width: none; transform: translateX(-50%)">'
RIGHT = 'right: 16px; top: 96px'
LEFT = 'left: 16px; top: 96px; width: 280px'


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
PARAM_BODY = (pair('Center', '30.0°E', '46.0°N') + pair('Parallels', '41.3°N', '50.7°N') + field('Rotation', '0.0°', True) + switch('Fit parallels to the frame')
              + '<h2>Distortion</h2><table class="t"><thead><tr><th></th><th>Scale</th><th>Area</th></tr></thead><tbody>'
              '<tr><td>North</td><td class="mono">+0.4%</td><td class="mono">+0.8%</td></tr><tr><td>Center</td><td class="mono">−0.3%</td><td class="mono">−0.6%</td></tr>'
              '<tr><td>South</td><td class="mono">+0.5%</td><td class="mono">+1.0%</td></tr></tbody></table>' + switch('Tissot circles')
              + '<h2>PROJ</h2><textarea class="in mono" rows="3" aria-label="PROJ string" style="width: 100%">+proj=lcc +lat_1=41.3 +lat_2=50.7 +lat_0=46 +lon_0=30 +R=4820000</textarea>')
DISTORT = '<g fill="#c2382b" fill-opacity=".14" stroke="#9c2415" stroke-width="3">' + CIRC + '</g>'
ipage('PadProjection', 'Vale projection on iPad', wswitch('Maps') + modes(MMODES, 'Projection', 'Map mode') + FRAME + actions()
      + SHEET + '<img class="lay" src="./plate-base.svg" alt="Map with distortion circles">'
      '<svg class="lay" viewBox="0 0 1500 2000"><g fill="#c2382b" fill-opacity=".14" stroke="#9c2415" stroke-width="3">' + CIRC + '</g></svg></div>'
      + panel('Projection', PROJ_LIST, LEFT, close=False) + panel('Parameters', PARAM_BODY, RIGHT, close=False))

ipage('PadStyle', 'Vale layer style in a map on iPad', wswitch('Maps') + modes(MMODES, 'Layout', 'Map mode') + FRAME + actions('Layers') + SHEET + PLATE % 'Map of Vesperan' + '</div>'
      + strip(PMTOOLS, 'Select') + panel('Topography', SCOPE + TOPO_STYLE, RIGHT + '; width: 320px', back='Layers'))

DATA_MARK = ('<svg class="lay" viewBox="0 0 1500 2000"><circle cx="700" cy="1060" r="9" fill="none" stroke="#0e8f76" stroke-width="1.6"></circle>'
             '<circle cx="700" cy="1060" r="4" fill="#45c4a8" stroke="#0b1f1a" stroke-width="1.2"></circle></svg>')
PLACE_BODY = ('<div class="row"><span class="lab">Name</span><input class="in" aria-label="Name"></div>' + pick('Kind', 'Town', 'City', 'Capital') + field('Rank', '3', True)
              + switch('Port', False) + '<div class="row"><span class="lab">Position</span><span class="mono mute">29.27°E 45.70°N</span></div>'
              '<div class="row"><button class="btn pri" style="flex: 1">Add</button></div>')


def places(rows):
    return ('<table class="t"><thead><tr><th>Name</th><th>Kind</th><th>Rank</th><th>Port</th></tr></thead><tbody>'
            + ''.join('<tr%s><td>%s</td><td>%s</td><td class="mono">%s</td><td class="mute">%s</td></tr>' % (
                (' class="sel"', '<span class="mute">New place</span>', k, r, p) if not n else ('', n, k, r, p)) for n, k, r, p in rows)
            + '</tbody></table>')


ipage('PadData', 'Vale data on iPad',
      '<div class="view bare" style="position: absolute; inset: 0; max-width: none"><div style="position: absolute; left: -950px; top: -1820px; width: 3000px; height: 4000px">'
      + PLATE % 'Map zoomed to Ostrey' + DATA_MARK + '</div></div>'
      + wswitch('Maps') + modes(MMODES, 'Data', 'Map mode') + FRAME + actions() + strip(PMTOOLS, 'Place point')
      + panel('New place', PLACE_BODY + '<h2>Snap to</h2>' + switch('Vertices') + switch('Lines'), RIGHT)
      + panel('Places', places(ROWS[1:6]), 'left: 16px; right: 332px; bottom: 16px; width: auto',
              act='<span class="mute" style="font-weight: 400; margin-right: 12px">58</span><input class="in" placeholder="Filter" aria-label="Filter" style="flex: 0 1 200px; margin-right: 8px">'), ink=True)

PAGES_BODY = pg(1, 'Kareth') + pg(2, 'Vesperan', True) + pg(3, 'Dravenhold') + pg(4, 'Marrow Sea') + '<div class="row"><button class="btn sec" style="flex: 1">Add a page</button></div>'
PAGE_BODY = (field('Title', 'Atlas of Kareth') + field('Imprint', 'Meridian Survey')
             + '<div class="row"><span class="lab">Paper</span><span class="sw" style="background: #f3efdf"></span><span class="mono mute">F3EFDF</span></div>'
             '<div class="row"><span class="lab">Border</span><span class="sw" style="background: #c9aaa0"></span><span class="mono mute">C9AAA0</span></div>'
             + switch('Locator globe') + switch('Scale bar') + switch('Page number', False))
ipage('PadAtlas', 'Vale atlas on iPad', wswitch('Atlas') + actions().replace(tbtn('Layers', LAYERS_IC) + tbtn('Toolbox', TOOLBOX), '<button class="btn pri">Export</button>')
      + SHEET.replace('sheet bare', 'sheet') + FULL % 'Atlas page for Vesperan' + '</div>'
      + panel('Pages', PAGES_BODY, LEFT, close=False) + panel('Page', PAGE_BODY, RIGHT, close=False)
      + READ.replace('24.6°E 31.2°N · 640 m', '12 × 16 in · 2 / 4') % 'left: 50%; bottom: 28px; transform: translateX(-50%)')
