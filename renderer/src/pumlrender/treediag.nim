## Cartes mentales (@startmindmap) et organigrammes de tâches (@startwbs).
import std/[strutils]
import common, preprocess

type TNode = ref object
  text: string
  line: int
  depth: int
  left: bool
  boxless: bool
  color: string
  children: seq[TNode]
  w, h, x, y, sub: float

proc parseTree(doc: Doc, ctx: Context): seq[TNode] =
  ## Renvoie les racines (normalement une seule).
  var stack: seq[TNode]
  var leftSide = false
  let lines = doc.lines
  var i = 0
  while i < lines.len:
    let l = lines[i]
    let t = l.text.strip()
    inc i
    if t.len == 0: continue
    let lw = t.toLowerAscii()
    if lw == "left side": (leftSide = true; continue)
    if lw == "right side": (leftSide = false; continue)
    if t[0] notin {'*', '+', '-', '#'}:
      ctx.warn(l.num, "ligne ignorée (marqueur *, +, - ou # attendu) : " & t)
      continue
    let marker = t[0]
    var d = 0
    while d < t.len and t[d] == marker: inc d
    var rest = t[d .. ^1]
    var boxless = false
    var color = ""
    if rest.startsWith("_"):
      boxless = true
      rest = rest[1 .. ^1]
    if rest.startsWith("[") :
      let e = rest.find(']')
      if e > 0:
        let opt = rest[1 ..< e]
        if opt.startsWith("#"): color = colorValue(opt)
        rest = rest[e + 1 .. ^1]
    rest = rest.strip()
    if rest.startsWith(":"):
      var buf = rest[1 .. ^1]
      while not buf.strip().endsWith(";") and i < lines.len:
        buf.add "\n" & lines[i].text.strip()
        inc i
      buf = buf.strip()
      if buf.endsWith(";"): buf.setLen(buf.len - 1)
      rest = buf
    let n = TNode(text: unescapeLabel(rest), line: l.num, depth: d, boxless: boxless, color: color,
      left: (marker == '-') or leftSide)
    while stack.len > 0 and stack[^1].depth >= d: discard stack.pop()
    if stack.len == 0:
      if d != 1 and result.len == 0: ctx.warn(l.num, "la racine devrait être au niveau 1")
      result.add n
    else:
      stack[^1].children.add n
    stack.add n

proc measureNode(n: TNode, root: bool) =
  let lines = n.text.split('\n')
  let size = if root: 15.0 else: FontSize
  n.w = blockWidth(lines, size, root) + (if n.boxless: 6.0 else: 24.0)
  n.h = blockHeight(lines.len, size) + (if n.boxless: 4.0 else: 14.0)
  for c in n.children: measureNode(c, false)

const VGap = 12.0
const HGap = 46.0

proc subH(n: TNode, kids: seq[TNode]): float =
  var s = 0.0
  for i, c in kids:
    s += c.sub
    if i > 0: s += VGap
  max(n.h, s)

proc layoutH(n: TNode) =
  for c in n.children: layoutH(c)
  n.sub = subH(n, n.children)

proc placeH(n: TNode, x, top: float, dir: float) =
  ## dir = 1 vers la droite, -1 vers la gauche ; x est le bord intérieur.
  n.y = top + n.sub / 2 - n.h / 2
  n.x = if dir > 0: x else: x - n.w
  var yy = top + (n.sub - subH(n, n.children)) / 2
  for c in n.children:
    let cx = if dir > 0: n.x + n.w + HGap else: n.x - HGap
    placeH(c, cx, yy, dir)
    yy += c.sub + VGap

proc drawNode(svg: var Svg, n: TNode, root: bool, ctx: Context) =
  let lines = n.text.split('\n')
  let size = if root: 15.0 else: FontSize
  if n.boxless:
    svg.line(n.x, n.y + n.h, n.x + n.w, n.y + n.h, Stroke, 1)
    svg.textBlock(n.x + 3, n.y + 1, lines, size = size, bold = root)
  else:
    let fill = if n.color.len > 0: n.color elif root: HeaderFill else: Fill
    svg.rect(n.x, n.y, n.w, n.h, fill, Stroke, (if root: 14.0 else: 8.0), (if root: 1.6 else: 1.2))
    svg.textBlock(n.x + 12, n.y + 7, lines, size = size, bold = root)
  ctx.region(n.line, n.x, n.y, n.w, n.h)

proc drawH(svg: var Svg, n: TNode, root: bool, ctx: Context) =
  for c in n.children:
    let toRight = c.x > n.x
    let x0 = if toRight: n.x + n.w else: n.x
    let y0 = n.y + n.h / 2
    let x1 = if toRight: c.x else: c.x + c.w
    let y1 = c.y + c.h / (if c.boxless: 1.0 else: 2.0)
    let mx = (x0 + x1) / 2
    svg.path("M" & fmtF(x0) & " " & fmtF(y0) & " C" & fmtF(mx) & " " & fmtF(y0) & " " & fmtF(mx) & " " &
      fmtF(y1) & " " & fmtF(x1) & " " & fmtF(y1), "none", Stroke, 1.2)
    drawH(svg, c, false, ctx)
  drawNode(svg, n, root, ctx)

proc layoutV(n: TNode) =
  for c in n.children: layoutV(c)
  var s = 0.0
  for i, c in n.children:
    s += c.sub
    if i > 0: s += 20
  n.sub = max(n.w, s)

proc placeV(n: TNode, left, y: float) =
  n.x = left + n.sub / 2 - n.w / 2
  n.y = y
  var s = 0.0
  for i, c in n.children:
    s += c.sub
    if i > 0: s += 20
  var xx = left + (n.sub - s) / 2
  for c in n.children:
    placeV(c, xx, y + n.h + 40)
    xx += c.sub + 20

proc drawV(svg: var Svg, n: TNode, root: bool, ctx: Context) =
  if n.children.len > 0:
    let x0 = n.x + n.w / 2
    let y0 = n.y + n.h
    let midY = y0 + 20
    svg.line(x0, y0, x0, midY)
    var lo = x0
    var hi = x0
    for c in n.children:
      let cx = c.x + c.w / 2
      lo = min(lo, cx)
      hi = max(hi, cx)
      svg.line(cx, midY, cx, c.y)
    svg.line(lo, midY, hi, midY)
  for c in n.children: drawV(svg, c, false, ctx)
  drawNode(svg, n, root, ctx)

proc extent(n: TNode, minX, minY, maxX, maxY: var float) =
  minX = min(minX, n.x)
  minY = min(minY, n.y)
  maxX = max(maxX, n.x + n.w)
  maxY = max(maxY, n.y + n.h)
  for c in n.children: extent(c, minX, minY, maxX, maxY)

proc shift(n: TNode, dx, dy: float) =
  n.x += dx
  n.y += dy
  for c in n.children: shift(c, dx, dy)

proc renderTree*(doc: Doc, ctx: Context): (Svg, float, float) =
  let roots = parseTree(doc, ctx)
  var svg: Svg
  if roots.len == 0: return (svg, 0.0, 0.0)
  var offY = 20.0
  var maxW = 0.0
  for root in roots:
    measureNode(root, true)
    if doc.kind == dkWbs:
      layoutV(root)
      placeV(root, 0, 0)
    else:
      var rights, lefts: seq[TNode]
      for c in root.children:
        if c.left: lefts.add c else: rights.add c
      for c in root.children: layoutH(c)
      let rh = subH(root, rights)
      let lh = subH(root, lefts)
      let total = max(rh, lh)
      root.x = 0
      root.y = total / 2 - root.h / 2
      var yy = (total - rh) / 2
      for c in rights:
        placeH(c, root.w + HGap, yy, 1)
        yy += c.sub + VGap
      yy = (total - lh) / 2
      for c in lefts:
        placeH(c, -HGap, yy, -1)
        yy += c.sub + VGap
    var minX, minY = Inf
    var maxX, maxY = -Inf
    extent(root, minX, minY, maxX, maxY)
    shift(root, 20 - minX, offY - minY)
    if doc.kind == dkWbs: drawV(svg, root, true, ctx) else: drawH(svg, root, true, ctx)
    maxW = max(maxW, maxX - minX + 40)
    offY += maxY - minY + 30
  (svg, maxW, offY - 10)
