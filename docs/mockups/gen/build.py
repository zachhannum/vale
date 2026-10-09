"""Writes the seven artboards. Quiet version: short labels, no helper text."""
import sys

OUT = sys.argv[1]
ON = ' checked="{{ true }}"'


def page(name, title, tab, body):
    tabs = ''.join('<a class="tab%s" href="%s">%s</a>' % (' on' if t == tab else '', h, t)
                   for t, h in (('World', 'Import.dc.html'), ('Maps', 'Main.dc.html'), ('Atlas', 'Atlas.dc.html')))
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
a{color:#45c4a8}a:hover{color:#8fe0cd}
</style>
</helmet>
<div class="app">
<header class="top">
<div class="brand">Vale</div>
<nav class="tabs" aria-label="Workspace">%s</nav>
<div class="grow"></div>
<div class="mono mute">kareth.gpkg</div>
</header>
%s
</div>
</x-dc>
<script type="text/x-dc" data-dc-script data-props='{"$preview":{"width":1440,"height":1000}}'>
class Component extends DCLogic {
renderVals() {
return {};
}
}
</script>
</body>
</html>
''' % (title, tabs, body)
    open('%s/%s.dc.html' % (OUT, name), 'w').write(html)


def sub(mode):
    seg = ''.join('<a%s href="%s.dc.html">%s</a>' % (' class="on"' if m == mode else '', f, m)
                  for m, f in (('Layout', 'Main'), ('Projection', 'Projection'), ('Style', 'Style'), ('Labels', 'Labels'), ('Data', 'Data')))
    return ('<div class="sub"><select class="in" aria-label="Map frame" style="flex: 0 1 200px"><option>Vesperan</option>'
            '<option>Kareth, the world</option><option>Dravenhold</option><option>Marrow Sea</option></select>'
            '<div class="grow"></div><nav class="seg" aria-label="Map mode">%s</nav></div>' % seg)


TOOLS = (('Select', '<path d="M5 3l14 8-6 2-2 6z"></path>'),
         ('Pan', '<path d="M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3"></path>'),
         ('Place point', '<path d="M12 21s-6-6-6-11a6 6 0 0112 0c0 5-6 11-6 11z"></path><circle cx="12" cy="10" r="2"></circle>'),
         ('Edit vertices', '<path d="M5 18l5-10 5 6 4-8"></path><circle cx="5" cy="18" r="1.5"></circle><circle cx="10" cy="8" r="1.5"></circle><circle cx="15" cy="14" r="1.5"></circle>'),
         ('Edit labels', '<path d="M6 6h12M12 6v13M9 19h6"></path>'))


def rail(on):
    return '<div class="rail" role="toolbar" aria-label="Tools">%s</div>' % ''.join(
        '<button class="tool%s" aria-label="%s"><svg viewBox="0 0 24 24">%s</svg></button>' % (' on' if n == on else '', n, p) for n, p in TOOLS)


def sec(title, inner, extra=''):
    return '<section class="sec"%s>%s%s</section>' % (extra, '<h2>%s</h2>' % title if title else '', inner)


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


def status(*parts):
    return '<div class="status">%s</div>' % ''.join('<div class="grow"></div>' if p is None else '<span class="mono">%s</span>' % p for p in parts)


PLATE = '<img class="lay" src="./plate-base.svg" alt="%s"><img class="lay" src="./plate-labels.svg" alt="">'

# 1 World
page('Import', 'Vale world', 'World', '<div class="body"><aside class="side l">'
     + sec('World', field('Name', 'Kareth') + field('Radius', '4,820 km', True)
           + '<div class="row"><span class="lab">Degree</span><span class="mono mute">84.1 km</span></div>')
     + sec('Sources', item('kareth-topography.geojson') + item('kareth-lines.svg')
           + item('coast-scan.png', 'No extents', True, cls='warn')
           + item('dravenhold-coast.psd', 'Missing', cls='bad')
           + '<div class="row" style="margin-top: 8px"><button class="btn">Add source</button></div>')
     + '</aside><main class="canvas"><div class="stage"><div class="view" style="aspect-ratio: 2 / 1; max-width: none">'
     '<img class="lay" src="./world-base.svg" alt="World map of Kareth with elevation bands">'
     '<svg class="lay" viewBox="0 0 1000 500" font-family="Helvetica, Arial, sans-serif" font-size="9" font-weight="bold">'
     '<g fill="none" stroke="#9c2415" stroke-width="1.2"><rect x="516.7" y="105.6" width="50" height="38.8"></rect>'
     '<rect x="558.3" y="102.8" width="50" height="38.9"></rect><rect x="588.9" y="88.9" width="55.5" height="38.9"></rect></g>'
     '<rect x="552.8" y="100" width="58.3" height="44.4" fill="none" stroke="#17191c" stroke-width="1.2" stroke-dasharray="4 2"></rect>'
     '<g fill="#17191c" stroke="#f3efdf" stroke-width="2.4" paint-order="stroke"><text x="514" y="156" text-anchor="end">Dravenhold</text>'
     '<text x="583" y="156" text-anchor="middle">Vesperan</text><text x="648" y="86">Marrow Sea</text></g></svg></div></div>'
     + status('32.04°E 46.98°N')
     + '</main><aside class="side r">'
     + sec('coast-scan.png',
           '<div style="position: relative; aspect-ratio: 21 / 16; overflow: hidden; background: #f4f1e6; margin-bottom: 12px">'
           '<img class="lay" src="/_blob/3de283ab580ffe42d27ea4cd6819505d" alt="Scanned coast sketch">'
           '<div class="pt" style="left: 23.8%; top: 25%">1</div><div class="pt" style="left: 81%; top: 75%">2</div></div>'
           '<div class="seg" role="group" aria-label="Extents method" style="margin-bottom: 10px"><button>Globe</button><button>Extents</button><button class="on">Points</button></div>'
           '<div class="row"><span class="lab">Point 1</span><input class="in mono" value="24°E" aria-label="Point 1 longitude"><input class="in mono" value="50°N" aria-label="Point 1 latitude"></div>'
           '<div class="row"><span class="lab">Point 2</span><input class="in mono" value="36°E" aria-label="Point 2 longitude"><input class="in mono" value="42°N" aria-label="Point 2 latitude"></div>'
           '<div class="row"><span class="lab">Extents</span><span class="mono mute">19–40°E · 38–54°N</span></div>'
           '<div class="row" style="margin-top: 12px; justify-content: flex-end"><button class="btn pri">Link</button></div>')
     + '</aside></div>')

# 2 Layout
LAYERS = (('Places', '#5a3229'), ('Peaks', '#4a3a32'), ('Sea routes', '#2c5866'), ('Roads', '#86503f'), ('Province borders', '#bf5645'),
          ('National border', '#c98f88'), ('Rivers and lakes', '#3f8aa3'), ('Graticule', '#5d6e72'), ('Topography', '#e2dca6'), ('Bathymetry', '#b6d0d3'))
CORNERS = ((190, 210), (1330, 210), (1330, 900), (1480, 985), (1480, 1500), (1330, 1585), (1330, 1840), (190, 1840), (190, 1000), (20, 915), (20, 480), (190, 395))
page('Main', 'Vale map layout', 'Maps', sub('Layout') + '<div class="body"><aside class="side l">'
     + sec('Map frames', item('Kareth, the world') + item('Vesperan', sel=True) + item('Dravenhold') + item('Marrow Sea'))
     + sec('Layers', ''.join('<label class="row"><input class="chk" type="checkbox"%s><span class="sw" style="background: %s"></span><span>%s</span></label>' % (ON, c, n) for n, c in LAYERS))
     + '</aside><main class="canvas"><div class="work">' + rail('Select')
     + '<div class="stage"><div class="sheet bare">' + PLATE % 'Map of Vesperan'
     + '<svg class="lay" viewBox="0 0 1500 2000" role="img" aria-label="Outline handles"><g fill="#45c4a8" stroke="#0b1f1a" stroke-width="3">'
     + ''.join('<rect x="%d" y="%d" width="20" height="20"></rect>' % (x - 10, y - 10) for x, y in CORNERS)
     + '</g></svg></div></div></div>' + status('29.27°E 45.70°N', None, '47%')
     + '</main><aside class="side r">'
     + sec('Vesperan', field('Name', 'Vesperan') + pick('Outline', 'Custom', 'Rectangle')
           + '<div class="row"><span class="lab">Projection</span><a href="Projection.dc.html">Conformal conic</a></div>'
           + field('Scale', '1:3,500,000', True) + pick('Page', '12 × 16 in', 'A3', 'Custom') + pick('Style', 'Tinted relief', 'Outline draft'))
     + sec('', '<div class="row"><span style="flex: 1">Labels</span><span class="mute">96</span></div>'
           '<div class="row"><a href="Labels.dc.html" style="flex: 1">Not placed</a><span class="warn">3</span></div>')
     + '</aside></div>')

# 3 Projection
CIRC = ''.join('<circle cx="%d" cy="%d" r="%s"></circle>' % (x, y, r) for y, r in ((420, 70.6), (820, 70.1), (1220, 70.1), (1620, 70.6)) for x in (420, 760, 1100))
page('Projection', 'Vale projection', 'Maps', sub('Projection') + '<div class="body"><aside class="side l">'
     + sec('Projection', item('Lambert conformal conic', '0.5%', True) + item('Transverse Mercator', '0.4%')
           + item('Albers equal-area conic', '0.9%') + item('Azimuthal equal-area', '0.3%') + item('Polar stereographic', '6%')
           + item('Mercator', '15%', cls='warn') + item('Orthographic') + item('Equal Earth') + item('Custom'))
     + '</aside><main class="canvas"><div class="work"><div class="stage"><div class="sheet bare">'
     '<img class="lay" src="./plate-base.svg" alt="Map with distortion circles">'
     '<svg class="lay" viewBox="0 0 1500 2000"><g fill="#c2382b" fill-opacity=".14" stroke="#9c2415" stroke-width="3">' + CIRC + '</g></svg>'
     '</div></div></div>' + status('29.27°E 45.70°N', None, '47%')
     + '</main><aside class="side r">'
     + sec('Parameters', '<div class="row"><span class="lab">Center</span><input class="in mono" value="30.0°E" aria-label="Center longitude"><input class="in mono" value="46.0°N" aria-label="Center latitude"></div>'
           '<div class="row"><span class="lab">Parallels</span><input class="in mono" value="41.3°N" aria-label="Parallel 1"><input class="in mono" value="50.7°N" aria-label="Parallel 2"></div>'
           + field('Rotation', '0.0°', True) + check('Fit parallels to frame'))
     + sec('Distortion', '<table class="t"><thead><tr><th></th><th>Scale</th><th>Area</th></tr></thead><tbody>'
           '<tr><td>North</td><td class="mono">+0.4%</td><td class="mono">+0.8%</td></tr>'
           '<tr><td>Center</td><td class="mono">−0.3%</td><td class="mono">−0.6%</td></tr>'
           '<tr><td>South</td><td class="mono">+0.5%</td><td class="mono">+1.0%</td></tr></tbody></table>'
           + '<div style="margin-top: 8px">' + check('Tissot circles') + '</div>')
     + sec('PROJ', '<textarea class="in mono" rows="3" aria-label="PROJ string" style="width: 100%">+proj=lcc +lat_1=41.3 +lat_2=50.7 +lat_0=46 +lon_0=30 +R=4820000</textarea>')
     + '</aside></div>')

# 4 Style
RAMP = (('#a2b291', '0'), ('#b2bf99', '100'), ('#c8ce9f', '200'), ('#e2dca6', '400'), ('#e9d49c', '700'), ('#dcb98b', '1,000'), ('#cc9a7b', '1,400'), ('#bd7f6e', '1,800'))
page('Style', 'Vale style', 'Maps', sub('Style') + '<div class="body"><aside class="side l">'
     + sec('Styles', item('Places', sw='#5a3229') + item('Peaks', sw='#4a3a32') + item('Sea routes', sw='#2c5866') + item('Roads', sw='#86503f')
           + item('Province borders', sw='#bf5645') + item('National border', sw='#c98f88') + item('Rivers and lakes', sw='#3f8aa3')
           + item('Topography', sel=True, sw='#e2dca6') + item('Bathymetry', sw='#b6d0d3'))
     + '</aside><main class="canvas" style="background: #17191c">'
     + sec('Rules', '<table class="t"><tbody>'
           '<tr class="sel"><td style="width: 28px"><input class="chk" type="checkbox"%s aria-label="Land on"></td><td>Land</td><td class="mono mute">elev_min &gt;= 0</td><td class="mute">Fill</td></tr>'
           '<tr><td><input class="chk" type="checkbox"%s aria-label="Coastline on"></td><td>Coastline</td><td class="mono mute">elev_min = 0</td><td class="mute">Stroke</td></tr>'
           '</tbody></table>' % (ON, ON))
     + sec('Land', '<div style="max-width: 520px">' + field('Filter', 'elev_min &gt;= 0', True)
           + '<div class="row"><span class="lab">Fill</span><div class="seg" role="group" aria-label="Fill kind"><button class="on">Color</button><button>Pattern</button><button>Texture</button></div></div>'
           + field('Color by', 'elev_min', True)
           + '<div style="display: flex; gap: 2px; margin: 12px 0 0 100px">'
           + ''.join('<div style="flex: 1; min-width: 0"><div style="height: 36px; background: %s"></div><div class="mono mute" style="margin-top: 4px; font-size: 11px">%s</div></div>' % r for r in RAMP)
           + '</div></div>', ' style="flex: 1"')
     + '</main><aside class="side r" style="flex-basis: 360px">'
     + sec('', '<div class="sheet bare" style="box-shadow: none; max-width: 330px; margin: 0 auto">' + PLATE % 'Style preview' + '</div>')
     + '</aside></div>')

# 5 Labels
CLASSES = ('Countries', 'Seas', 'Provinces', 'Cities', 'Towns', 'Ranges', 'Rivers', 'Peaks', 'Islands', 'Sea routes')
page('Labels', 'Vale labels', 'Maps', sub('Labels') + '<div class="body"><aside class="side l">'
     + sec('Label classes', ''.join(item(c, {'Towns': '1', 'Rivers': '1', 'Islands': '1'}.get(c, ''), c == 'Cities', cls='warn') for c in CLASSES))
     + sec('Cities', field('Text', 'name', True) + field('Filter', 'rank &lt;= 2', True) + pick('Font', 'Serif small caps') + field('Size', '7 pt', True)
           + pick('Position', 'Around symbol', 'On symbol'))
     + sec('Fallbacks', check('Shrink to 6 pt') + check('Stack') + check('Short name', False) + check('Leader line'))
     + '</aside><main class="canvas"><div class="work">' + rail('Edit labels')
     + '<div class="stage"><div class="view bare" style="aspect-ratio: 3 / 2"><div class="zoom" style="width: 277.8%; left: -127.8%; top: -230.6%">'
     + PLATE % 'Map zoomed to Castellane'
     + '<svg class="lay" viewBox="0 0 1500 2000"><g fill="#0f4a3e" fill-opacity=".1" stroke="#0f4a3e" stroke-width=".8" stroke-dasharray="3 2">'
     '<rect x="956" y="1013" width="77" height="12.5"></rect><rect x="867" y="995" width="77" height="12.5"></rect><rect x="867" y="1013" width="77" height="12.5"></rect></g>'
     '<rect x="954.5" y="993.5" width="80" height="15" fill="none" stroke="#0e8f76" stroke-width="1.6"></rect>'
     '<rect x="890" y="885.5" width="66" height="15" fill="none" stroke="#b36b00" stroke-width="1.2"></rect>'
     '<rect x="769.5" y="924" width="30.5" height="13" fill="none" stroke="#b36b00" stroke-width="1.2"></rect></svg></div>'
     '<div role="toolbar" aria-label="Label actions" class="float" style="left: 45%; top: 61%"><button class="btn">Pin</button><button class="btn">Next</button><button class="btn">Hide</button></div>'
     '</div></div></div>' + status('31.42°E 46.31°N', None, '130%')
     + '</main><aside class="side r">'
     + sec('Castellane', '<table class="t"><tbody>'
           '<tr class="sel"><td>Northeast</td><td class="mono">0.22</td></tr><tr><td>Southeast</td><td class="mono mute">0.47</td></tr>'
           '<tr><td>Northwest</td><td class="mono mute">0.66</td></tr><tr><td>Southwest</td><td class="mono mute">0.81</td></tr></tbody></table>')
     + sec('Overrides', item('Deepwell', 'Pinned') + item('Tarn', 'Moved') + item('Greywater', 'Moved') + item('Sable Ford', 'Hidden'))
     + sec('Not placed', item('Callow Ferry', 'Overlap') + item('Little Sable', 'Too long') + item('Skerry Rocks', 'Overlap'))
     + '</aside></div>')

# 6 Data
ROWS = (('Ardent', 'Town', '3', ''), ('Castellane', 'City', '2', 'Port'), ('Hartle', 'Town', '3', ''), ('', 'Town', '3', ''),
        ('Ostrey', 'Town', '3', ''), ('Tolver', 'Town', '3', ''), ('Yarrow', 'Town', '3', 'Port'))
page('Data', 'Vale data', 'Maps', sub('Data') + '<div class="body"><main class="canvas"><div class="work" style="min-height: 440px">' + rail('Place point')
     + '<div class="stage" style="padding: 16px"><div class="view bare" style="aspect-ratio: 9 / 5; max-width: 860px"><div class="zoom" style="width: 277.8%; left: -98.1%; top: -323.3%">'
     + PLATE % 'Map zoomed to Ostrey'
     + '<svg class="lay" viewBox="0 0 1500 2000"><circle cx="700" cy="1060" r="9" fill="none" stroke="#0e8f76" stroke-width="1.6"></circle>'
     '<circle cx="700" cy="1060" r="4" fill="#45c4a8" stroke="#0b1f1a" stroke-width="1.2"></circle></svg></div></div></div></div>'
     '<div class="sub" style="border-top: 1px solid #2a2e34"><strong>Places</strong><span class="mute">58</span><div class="grow"></div>'
     '<input class="in" placeholder="Filter" aria-label="Filter" style="flex: 0 1 220px"></div>'
     '<div class="scroll" style="background: #1f2226"><table class="t"><thead><tr><th>Name</th><th>Kind</th><th>Rank</th><th>Port</th></tr></thead><tbody>'
     + ''.join('<tr%s><td>%s</td><td>%s</td><td class="mono">%s</td><td class="mute">%s</td></tr>' % (
         (' class="sel"', '<span class="mute">New place</span>', k, r, p) if not n else ('', n, k, r, p)) for n, k, r, p in ROWS)
     + '</tbody></table></div>' + status('29.27°E 45.70°N', None, '130%')
     + '</main><aside class="side r">'
     + sec('New place', '<div class="row"><span class="lab">Name</span><input class="in" aria-label="Name"></div>'
           + pick('Kind', 'Town', 'City', 'Capital') + field('Rank', '3', True) + check('Port', False)
           + '<div class="row"><span class="lab">Position</span><span class="mono mute">29.27°E 45.70°N</span></div>'
           '<div class="row" style="margin-top: 12px; justify-content: flex-end"><button class="btn pri">Add</button></div>')
     + sec('Snap', check('Vertices') + check('Lines'))
     + '</aside></div>')

# 7 Atlas
FULL = '<img class="lay" src="./page-under.svg" alt=""><img class="lay" src="./plate-base.svg" alt="%s"><img class="lay" src="./plate-labels.svg" alt=""><img class="lay" src="./page-over.svg" alt="">'


def pg(n, name, sel=False):
    return '<div class="row%s" style="gap: 12px; padding-top: 6px; padding-bottom: 6px"><span class="mono mute">%d</span><div class="thumb">%s</div><span>%s</span></div>' % (
        ' sel' if sel else '', n, FULL % '' if sel else '', name)


page('Atlas', 'Vale atlas', 'Atlas', '<div class="body"><aside class="side l">'
     + sec('Pages', pg(1, 'Kareth') + pg(2, 'Vesperan', True) + pg(3, 'Dravenhold') + pg(4, 'Marrow Sea'))
     + '</aside><main class="canvas"><div class="stage"><div class="sheet" style="max-width: 600px">' + FULL % 'Atlas page for Vesperan'
     + '</div></div>' + status('12 × 16 in', None, '2 / 4')
     + '</main><aside class="side r">'
     + sec('Page', field('Title', 'Atlas of Kareth') + field('Imprint', 'Meridian Survey')
           + '<div class="row"><span class="lab">Paper</span><span class="sw" style="background: #f3efdf"></span><span class="mono mute">F3EFDF</span></div>'
           '<div class="row"><span class="lab">Border</span><span class="sw" style="background: #c9aaa0"></span><span class="mono mute">C9AAA0</span></div>'
           + check('Locator globe') + check('Scale bar') + check('Page number', False))
     + sec('Export', '<div class="seg" role="group" aria-label="Format" style="margin-bottom: 10px"><button class="on">PDF</button><button>SVG</button><button>PNG</button><button>TIFF</button></div>'
           + pick('Pages', 'All', 'This page')
           + '<div class="row" style="margin-top: 12px"><button class="btn pri" style="flex: 1">Export</button></div>')
     + '</aside></div>')


# World sub bar, Draw board, and iPad boards
def wsub(mode):
    return '<div class="sub"><div class="grow"></div><nav class="seg" aria-label="World mode">%s</nav></div>' % ''.join(
        '<a%s href="%s.dc.html">%s</a>' % (' class="on"' if m == mode else '', f, m) for m, f in (('Draw', 'Draw'), ('Sources', 'Import')))


DTOOLS = (('Raise', '<path d="M3 18c4 0 5-11 9-11s5 11 9 11"></path>'), ('Lower', '<path d="M3 7c4 0 5 11 9 11s5-11 9-11"></path>'),
          ('Smooth', '<path d="M3 12c3-4 6-4 9 0s6 4 9 0"></path>'), ('Flatten', '<path d="M4 12h16"></path>'),
          ('Line', '<path d="M4 20l4-1 11-11-3-3L5 16z"></path>'), TOOLS[1])


def tools(cls, set_, on):
    return '<div class="%s" role="toolbar" aria-label="Tools">%s</div>' % (cls, ''.join(
        '<button class="tool%s" aria-label="%s"><svg viewBox="0 0 24 24">%s</svg></button>' % (' on' if n == on else '', n, p) for n, p in set_))


def slider(label, value, pct):
    return ('<div class="row"><span class="lab">%s</span><div class="sl" role="slider" aria-label="%s" aria-valuenow="%d" aria-valuemin="0" aria-valuemax="100" tabindex="0">'
            '<div style="width: %d%%"></div></div><span class="mono mute" style="flex: 0 0 52px; text-align: right">%s</span></div>' % (label, label, pct, pct, value))


GLOBE = ('<img class="lay" src="./globe.svg" alt="Globe of Kareth with elevation bands">'
         '<svg class="lay" viewBox="0 0 1000 1000"><circle cx="388" cy="402" r="46" fill="none" stroke="#17191c" stroke-width="1.6"></circle>'
         '<circle cx="388" cy="402" r="24" fill="none" stroke="#17191c" stroke-width="1" stroke-dasharray="3 3"></circle></svg>')

# rebuild Import with the World sub bar
src = open(OUT + '/Import.dc.html').read()
open(OUT + '/Import.dc.html', 'w').write(src.replace('</header>\n<div class="body">', '</header>\n' + wsub('Sources') + '<div class="body">'))

page('Draw', 'Vale draw', 'World', wsub('Draw') + '<div class="body"><aside class="side l">'
     + sec('Layers', item('Height', sel=True) + item('Rivers') + item('Borders') + item('Places'))
     + sec('Bands', ''.join('<div class="row"><span class="sw" style="background: %s"></span><span class="mono">%s</span></div>' % r for r in reversed(RAMP)))
     + '</aside><main class="canvas"><div class="work">' + tools('rail', DTOOLS, 'Raise')
     + '<div class="stage"><div class="globe">' + GLOBE + '</div></div></div>' + status('24.6°E 31.2°N', '640 m', None, '100%')
     + '</main><aside class="side r">'
     + sec('Brush', slider('Size', '120 km', 40) + slider('Softness', '80%', 80) + slider('Flow', '30%', 30) + check('Pen pressure'))
     + sec('View', '<div class="seg" role="group" aria-label="View"><button class="on">Bands</button><button>Grey</button></div>')
     + sec('Toolbox', '<div class="row"><input class="in" placeholder="Search" aria-label="Search tools"></div>'
           + ''.join(item(t) for t in ('Polygonize', 'Contour lines', 'Smooth', 'Simplify', 'Dissolve', 'Clip')))
     + '</aside></div>')


def ipage(name, title, tab, inner):
    tabs = ''.join('<a%s href="%s">%s</a>' % (' class="on"' if t == tab else '', h, t)
                   for t, h in (('World', 'PadDraw.dc.html'), ('Maps', 'PadMap.dc.html'), ('Atlas', 'PadMap.dc.html')))
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
<div class="app pad">
<nav class="seg card" aria-label="Workspace" style="left: 16px; top: 16px">%s</nav>
%s
</div>
</x-dc>
<script type="text/x-dc" data-dc-script data-props='{"$preview":{"width":1194,"height":834}}'>
class Component extends DCLogic {
renderVals() {
return {};
}
}
</script>
</body>
</html>
''' % (title, tabs, inner)
    open('%s/%s.dc.html' % (OUT, name), 'w').write(html)


def pmode(mode):
    return '<nav class="seg card" aria-label="Map mode" style="left: 50%%; top: 16px; transform: translateX(-50%%)">%s</nav>' % ''.join(
        '<a%s href="%s.dc.html">%s</a>' % (' class="on"' if m == mode else '', f, m)
        for m, f in (('Layout', 'PadMap'), ('Projection', 'PadMap'), ('Style', 'PadMap'), ('Labels', 'PadLabels'), ('Data', 'PadMap')))


def vslider(label, pct, top):
    return ('<div class="vs card" role="slider" aria-label="%s" aria-orientation="vertical" aria-valuenow="%d" aria-valuemin="0" aria-valuemax="100" tabindex="0" '
            'style="left: 16px; top: %dpx"><div style="height: %d%%"></div></div>' % (label, pct, top, pct))


FRAME = '<div class="card pill" style="right: 16px; top: 16px">Vesperan</div>'

ipage('PadDraw', 'Vale draw on iPad', 'World',
      '<div class="globe" style="position: absolute; left: 50%; top: 50%; width: 760px; transform: translate(-50%, -50%)">' + GLOBE + '</div>'
      + tools('card ptools', DTOOLS, 'Raise').replace('class="card ptools"', 'class="card ptools" style="left: 16px; top: 84px"')
      + vslider('Size', 40, 420) + vslider('Flow', 30, 620).replace('left: 16px', 'left: 16px')
      + '<div class="seg card" role="group" aria-label="View" style="right: 16px; top: 16px"><button class="on">Bands</button><button>Grey</button></div>'
      + '<button class="card pill" aria-label="Toolbox" style="right: 16px; top: 76px; padding: 0 12px"><svg viewBox="0 0 24 24" style="width: 21px; height: 21px; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round"><path d="M4 9h16v10H4zM9 9V6h6v3M4 14h16"></path></svg></button>'
      + '<div class="mono mute" style="position: absolute; left: 50%; bottom: 20px; transform: translateX(-50%)">24.6°E 31.2°N · 640 m</div>')

ipage('PadMap', 'Vale map on iPad', 'Maps', pmode('Layout') + FRAME
      + '<div class="sheet bare" style="position: absolute; left: 50%; top: 84px; width: 548px; max-width: none; transform: translateX(-50%)">' + PLATE % 'Map of Vesperan'
      + '<svg class="lay" viewBox="0 0 1500 2000" role="img" aria-label="Outline handles"><g fill="#45c4a8" stroke="#0b1f1a" stroke-width="3">'
      + ''.join('<circle cx="%d" cy="%d" r="16"></circle>' % c for c in CORNERS) + '</g></svg></div>'
      + tools('card ptools', TOOLS, 'Select').replace('class="card ptools"', 'class="card ptools" style="left: 16px; top: 84px"')
      + '<div class="card panel" style="right: 16px; top: 84px">' + field('Name', 'Vesperan')
      + '<div class="row"><span class="lab">Projection</span><span>Conformal conic</span></div>' + field('Scale', '1:3,500,000', True) + pick('Page', '12 × 16 in')
      + pick('Style', 'Tinted relief') + '</div>')

ipage('PadLabels', 'Vale labels on iPad', 'Maps',
      '<div class="view bare" style="position: absolute; inset: 0; max-width: none"><div style="position: absolute; left: -1300px; top: -1603px; width: 3000px; height: 4000px">'
      + PLATE % 'Map zoomed to Castellane'
      + '<svg class="lay" viewBox="0 0 1500 2000"><g fill="#0f4a3e" fill-opacity=".1" stroke="#0f4a3e" stroke-width=".8" stroke-dasharray="3 2">'
      '<rect x="956" y="1013" width="77" height="12.5"></rect><rect x="867" y="995" width="77" height="12.5"></rect><rect x="867" y="1013" width="77" height="12.5"></rect></g>'
      '<rect x="954.5" y="993.5" width="80" height="15" fill="none" stroke="#0e8f76" stroke-width="1.6"></rect>'
      '<rect x="890" y="885.5" width="66" height="15" fill="none" stroke="#b36b00" stroke-width="1.2"></rect></svg></div></div>'
      + pmode('Labels') + FRAME
      + tools('card ptools', TOOLS, 'Edit labels').replace('class="card ptools"', 'class="card ptools" style="left: 16px; top: 84px"')
      + '<div role="toolbar" aria-label="Label actions" class="card ptools" style="left: 590px; top: 440px; flex-direction: row"><button class="btn">Pin</button><button class="btn">Next</button><button class="btn">Hide</button></div>'
      + '<div class="card panel" style="right: 16px; top: 84px; width: 220px"><h2>Not placed</h2>' + item('Callow Ferry') + item('Little Sable') + item('Skerry Rocks') + '</div>')
