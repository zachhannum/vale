"""Writes the iPhone screen files. Run it after build.py, because it imports build.py."""
from build import *

# iPhone boards. All sizes are in points, for an iPhone of 393 by 852 points.
# In portrait, the top 59 points hold the status bar and the Dynamic Island, and the bottom 34 points hold the home indicator.
# In landscape, the system hides the status bar, and the left 59 points, the right 59 points, and the bottom 21 points are outside the safe area.
PHONE_CSS = '''.phone .sys{height:54px;padding:4px 34px 0 50px;font-size:16px;font-weight:600}
.phone .home{width:134px;margin-left:-67px}
.phone .island{position:absolute;z-index:5;left:50%;top:11px;width:126px;height:37px;margin-left:-63px;border-radius:19px;background:#000}
.phone.land .island{left:11px;top:50%;width:37px;height:126px;margin:-63px 0 0}
.phone .ptools.h{flex-direction:row}
.phone .hs{flex:1;display:flex;flex-direction:column;justify-content:center;gap:14px;padding:0 12px;font-size:11px;color:var(--mute)}
.phone .hs .hl{display:flex;justify-content:space-between}
.phone .hs .mono{font-size:11px}
.phone .hs .sl{flex:none;height:6px}
.phone .sheetp{border-radius:22px 22px 0 0}
.phone .sheetp .pb{overflow:hidden}
.phone .dim{position:absolute;inset:0;z-index:2;background:rgba(0,0,0,.35)}
.phone .t{font-size:14px}
.phone .t td,.phone .t th{padding:10px 8px}
.phone .big{position:absolute;left:16px;top:60px;font-size:32px;font-weight:700;letter-spacing:.2px}
.phone .grid{position:absolute;left:16px;right:16px;top:176px;display:grid;grid-template-columns:1fr 1fr;gap:20px 14px}
.phone .pc .pic{position:relative;display:grid;place-items:center;aspect-ratio:1;border-radius:14px;background:#1b1e23;box-shadow:inset 0 0 0 1px #2a2e34}
.phone .pc.hov .pic{box-shadow:inset 0 0 0 2px var(--acc)}
.phone .pc .pic img{width:78%;aspect-ratio:1}
.phone .pc .badge{position:absolute;left:8px;top:8px;padding:2px 7px;border-radius:6px;background:rgba(0,0,0,.6);font-size:11px;color:var(--warn)}
.phone .pc .dl2{display:flex;flex-direction:column;align-items:center;gap:8px;font-size:12px;color:var(--mute)}
.phone .pc .dl2 svg{width:30px;height:30px;fill:none;stroke:var(--mute);stroke-width:1.4;stroke-linejoin:round}
.phone .pm{padding:8px 2px 0;font-size:12px;line-height:1.45;color:var(--mute)}
.phone .pn{font-size:14px;font-weight:600;color:var(--text)}
.phone .search{position:absolute;left:16px;right:16px;top:116px;height:40px;display:flex;align-items:center;gap:8px;padding:0 12px;border-radius:12px;background:rgba(255,255,255,.08);color:var(--mute)}
.phone .search svg{width:18px;height:18px;fill:none;stroke:currentColor;stroke-width:1.8;stroke-linecap:round}
.phone .kb{position:absolute;z-index:3;left:0;right:0;bottom:0;height:336px;padding:8px 3px 0;background:#2c2c2e;display:flex;flex-direction:column;gap:11px}
.phone .kr{display:flex;justify-content:center;gap:6px}
.phone .kr span{flex:0 0 33px;height:42px;display:grid;place-items:center;border-radius:5px;background:#6b6b6e;font-size:22px;box-shadow:0 1px 0 #000}
.phone .kr span.w{flex-basis:44px;background:#464648;font-size:15px}
.phone .kr span.sp{flex:1}
.phone .kr span.rt{flex-basis:92px;background:#464648;font-size:15px}'''


def phone(name, title, inner, land=False, ink=False):
    w, h = (852, 393) if land else (393, 852)
    sys_bar = '' if land else '<div class="sys%s" aria-hidden="true"><span>9:41</span>%s</div>' % (' ink' if ink else '', BATTERY)
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
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono&family=IBM+Plex+Sans:wght@400;500;600;700&display=swap">
<style>
body{margin:0}
a{color:#45c4a8}
%s
</style>
</helmet>
<div class="app pad phone%s" style="height: %dpx">
%s<div class="island" aria-hidden="true"></div>
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
''' % (title, PHONE_CSS, ' land' if land else '', h, sys_bar, inner, ' ink' if ink else '', w, h)
    open('%s/%s.dc.html' % (OUT, name), 'w').write(html)


# The phone has no pen. With a brush tool, one finger paints, and two fingers turn and zoom the globe. With Move, one finger turns the globe.
# The top row holds the workspace menu at the left and the actions at the right. The view switch is in the workspace menu.
TOP = 'top: 59px'


def pmenu_btn(tab, on=False, style='left: 12px; ' + TOP):
    return wmenu(tab, on).replace('left: 16px; top: 32px', style)


def pacts(on=None, style='right: 12px; ' + TOP):
    return actions(on).replace('right: 16px; top: 32px', style)


# The tool strip is a row at the bottom, in reach of the thumb. The brush card is a row above it.
def hstrip(set_, on, style='left: 50%; bottom: 42px; transform: translateX(-50%)'):
    return strip(set_, on).replace('class="card ptools"', 'class="card ptools h"').replace('left: 16px; top: 96px', style)


def hslider(label, value, pct):
    return ('<div class="hs" role="slider" aria-label="%s" aria-valuenow="%d" aria-valuemin="0" aria-valuemax="100" tabindex="0">'
            '<div class="hl"><span>%s</span><span class="mono">%s</span></div><div class="sl"><div style="width: %d%%"><i></i></div></div></div>' % (label, pct, label, value, pct))


def hbrush(on=False, first=('Size', '120 km', 40), style='left: 50%; bottom: 110px; width: 344px; transform: translateX(-50%)'):
    return ('<div class="card ptools h" role="group" aria-label="Brush" style="%s">' % style + hslider(*first) + hslider('Flow', '30%', 30)
            + '<button class="tool%s" aria-label="Brush settings">%s<span>Brush</span></button></div>' % (' on' if on else '', ic(MORE)))


PREAD = READ % 'left: 50%; bottom: 178px; transform: translateX(-50%)'
PGLOBE = globe('left: 50%; top: 400px; width: 640px; transform: translate(-50%, -50%)')
PDRAW = PGLOBE + pmenu_btn('World') + hstrip(PDTOOLS, 'Raise')


def psheet(title, body, height, act='', back=None):
    s = sheet(title, body, height)
    if act:
        s = s.replace(tbtn('Close', CLOSE), act + tbtn('Close', CLOSE))
    if back:
        s = s.replace('<div class="ph"><span class="grow">', '<div class="ph bk">%s<span class="grow">' % tbtn('Back to ' + back, BACK))
    return s


phone('PhoneDraw', 'Vale draw on iPhone', PDRAW + pacts() + hbrush() + PREAD)
phone('PhoneDrawMenu', 'Vale workspace menu on iPhone', PDRAW + pacts() + hbrush() + PREAD
      + panel('Workspace', MENU_BODY + RULE + '<div class="row"><span class="grow">Worlds</span></div>', 'left: 12px; top: 119px; width: 240px; z-index: 3', close=False)
      .replace('class="card panel"', 'class="card panel menup"'))

# A panel is a sheet at the bottom. It opens at half height, and you pull it up to the full height or down to its title row.
phone('PhoneDrawLayers', 'Vale layers on iPhone', PDRAW + pacts('Layers') + psheet('Layers', LAYERS_BODY, 452, act=LAYERS_ADD))
# A finger has no pressure, so the Brush panel has no pressure settings.
PBRUSH_BODY = BRUSH_BODY[:BRUSH_BODY.index('<h2>Pen pressure')] + switch('Size follows zoom')
phone('PhoneDrawBrush', 'Vale brush settings on iPhone', PDRAW + pacts() + psheet('Brush', PBRUSH_BODY, 452))
phone('PhoneDrawStyle', 'Vale layer style on iPhone', PGLOBE + pmenu_btn('World') + hstrip(PDTOOLS, 'Line') + pacts('Layers')
      + '<div class="dim"></div>' + psheet('Rivers', RIVER_STYLE, 728, back='Layers'))

# Landscape. The tool strip is a column at the left edge, and the brush card holds the Brush button alone.
# The width is 600 points or more, so the view switch is in the top row, and a panel floats at the right.
LTOOLS = strip(PDTOOLS, 'Raise').replace('left: 16px; top: 96px', 'left: 59px; top: 12px').replace('class="tool', 'style="height: 46px" class="tool')
LBRUSH = ('<div class="card ptools" role="group" aria-label="Brush" style="left: 59px; top: 304px">'
          '<button class="tool" style="height: 46px" aria-label="Brush settings">%s<span>Brush</span></button></div>' % ic(MORE))
phone('PhoneDrawLand', 'Vale draw on iPhone in landscape',
      globe('left: 426px; top: 50%; width: 560px; transform: translate(-50%, -50%)') + LTOOLS + LBRUSH
      + pmenu_btn('World', style='left: 131px; top: 12px') + modes(DMODES, 'Globe', 'View').replace('top: 32px', 'top: 12px')
      + pacts('Layers', 'right: 59px; top: 12px') + READ % 'left: 131px; bottom: 21px'
      + panel('Layers', LAYERS_BODY, 'right: 59px; top: 76px; bottom: 21px; width: 300px; overflow: hidden', act=LAYERS_ADD), land=True)


# Atlas. The page controls are below the top row, and the tools are at the bottom.
def ppage(over=''):
    return ('<div class="sheet" style="position: absolute; left: 12px; top: 175px; width: 369px; max-width: none">'
            + FULL % 'Atlas page for Vesperan' + over + '</div>')


PPAGE_CTL = PAGE_CTL.replace('top: 32px', 'top: 119px')
PATLAS = pmenu_btn('Atlas') + PPAGE_CTL + pacts().replace(tbtn('Layers', LAYERS_IC) + tbtn('Toolbox', TOOLBOX), tbtn('Add', PLUS) + '<button class="btn pri">Export</button>')
phone('PhoneAtlas', 'Vale atlas page on iPhone', PATLAS + ppage() + hstrip(PATOOLS, 'Select'))
phone('PhoneAtlasFrame', 'Vale map frame on iPhone', PATLAS + ppage(fhandles(INSET, 56)) + hstrip(PATOOLS, 'Select')
      + psheet('Kareth globe', ptabs(FTABS, 'Frame') + FRAME_TAB, 452))


# The flat view and the attribute table. The table is a sheet with fewer columns.
def flat_at(over, w, left, top):
    return ('<div class="view" style="position: absolute; left: %dpx; top: %dpx; width: %dpx; max-width: none; aspect-ratio: 2 / 1">' % (left, top, w)
            + '<img class="lay" src="./world-base.svg" alt="Flat map of Kareth with elevation bands"><svg class="lay" viewBox="0 0 1000 500">' + over + '</svg></div>')


PDATA = pmenu_btn('World') + hstrip(PPOINT, 'Place point')
FILTER = '<div class="row"><input class="in" placeholder="Filter" aria-label="Filter"><span class="mute">58</span></div>'
phone('PhoneData', 'Vale attribute table on iPhone', flat_at(DATA_MARK, 1600, -733, 140) + PDATA + pacts('Layers')
      + psheet('Places', FILTER + places(ROWS), 452))

# The form of a new point is a card below the top row. The keyboard covers the bottom, and the point is between them.
KEYS = ('<div class="kb" aria-hidden="true">'
        + '<div class="kr">%s</div>' % ''.join('<span>%s</span>' % k for k in 'qwertyuiop')
        + '<div class="kr">%s</div>' % ''.join('<span>%s</span>' % k for k in 'asdfghjkl')
        + '<div class="kr"><span class="w">⇧</span>%s<span class="w">⌫</span></div>' % ''.join('<span>%s</span>' % k for k in 'zxcvbnm')
        + '<div class="kr"><span class="w">123</span><span class="sp"></span><span class="rt">return</span></div></div>')
POINT_FORM = ('<div class="row"><span class="lab">Name</span><input class="in" value="Ferrow" aria-label="Name" style="box-shadow: inset 0 0 0 2px #45c4a8"></div>'
              + pick('Kind', 'Town', 'City', 'Capital') + field('Rank', '3', True) + switch('Port', False))
phone('PhonePoint', 'Vale new point on iPhone', flat_at(DATA_MARK, 1600, -733, 247) + PDATA + pacts()
      + panel('New place', POINT_FORM, 'left: 12px; right: 12px; top: 119px; width: auto; z-index: 3', close=False,
              act='<button class="btn">Cancel</button><button class="btn pri" style="margin-right: 4px">Add</button>') + KEYS)


# Start screen. The projects are a grid of two columns. A long press on a card opens its menu.
def pc(name, date, where, hue=0, pic=True, badge='', hov=False):
    img = ('<img src="./globe.svg" alt="" style="filter: hue-rotate(%ddeg)">' % hue) if pic else '<span class="dl2">%sNot downloaded</span>' % ic(CLOUD)
    return ('<article class="pc%s"><div class="pic">%s%s</div><div class="pm"><div class="pn">%s</div><div>%s · %s</div></div></article>'
            % (' hov' if hov else '', img, '<span class="badge">%s</span>' % badge if badge else '',
               name if pic else name.lower().replace(' ', '-') + '.gpkg', date, where))


START = ('<div class="big">Worlds</div>'
         '<div class="card bar" role="toolbar" aria-label="Actions" style="right: 12px; top: 59px">%s%s</div>' % (tbtn('Sort', LIST), tbtn('New world…', PLUS))
         + '<div class="search">%s<span>Search</span></div>' % ic(SEARCH)
         + '<div class="grid">' + pc('Kareth', 'Today', 'iPhone') + pc('Isles of Morrow', 'Oct 12', 'iCloud', 140, badge='2 versions', hov=True)
         + pc('The Sundering', 'Oct 3', 'iPhone', 260) + pc('Ashfall Reach', 'Sep 28', 'iCloud', 40)
         + pc('Tidewater', 'Sep 9', 'iPhone', 200) + pc('Low Countries', 'Aug 30', 'iCloud', pic=False) + '</div>')
phone('PhoneStart', 'Vale start screen on iPhone', START
      + panel('Isles of Morrow', prow('Open') + RULE + prow('Rename…') + prow('Duplicate') + prow('Show in Files') + RULE
              + '<div class="row"><span class="grow" style="color: var(--bad)">Delete…</span></div>',
              'right: 12px; top: 392px; width: 220px; z-index: 3', close=False).replace('class="card panel"', 'class="card panel menup"'))
