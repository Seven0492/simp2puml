## Schémas ASCII @startditaa : traits, angles, flèches, boîtes colorées et texte.
import std/[strutils, unicode]
import common, preprocess

const
  CellW = 9.0
  CellH = 17.0
  HLine = {'-', '=', '+', '/', '\\', '<', '>'}
  VLine = {'|', ':', '+', '/', '\\', '^', 'v', 'V', '*'}

proc colorTag(s: string): string =
  ## « cRED », « cBLU », « c1AB » : couleur de boîte de ditaa.
  case s
  of "RED": "#EE3322"
  of "BLU": "#3366FF"
  of "GRE": "#339933"
  of "YEL": "#FFCC33"
  of "PNK": "#FFAACC"
  of "BLK": "#000000"
  else:
    if s.len == 3:
      for c in s:
        if c notin HexDigits: return ""
      "#" & s[0] & s[0] & s[1] & s[1] & s[2] & s[2]
    else: ""

proc renderDitaa*(doc: Doc, ctx: Context): (Svg, float, float) =
  var svg: Svg
  var grid: seq[seq[Rune]]
  var nums: seq[int]
  for l in doc.lines:
    grid.add toRunes(l.text.replace("\t", "    "))
    nums.add l.num
  if grid.len == 0: return (svg, 0.0, 0.0)
  var cols = 0
  for r in grid: cols = max(cols, r.len)
  if cols * grid.len > 1_000_000:
    ctx.error(doc.startLine, "schéma ditaa trop grand")
    return (svg, 0.0, 0.0)
  proc at(y, x: int): char =
    if y < 0 or y >= grid.len or x < 0 or x >= grid[y].len: return ' '
    let r = grid[y][x]
    if int32(r) < 128: char(int32(r)) else: 'a'
  proc isH(y, x: int): bool =
    let c = at(y, x)
    if c == '+': return true
    if c notin {'-', '='}: return false
    at(y, x - 1) in HLine or at(y, x + 1) in HLine
  proc isV(y, x: int): bool =
    let c = at(y, x)
    if c == '+': return true
    if c notin {'|', ':'}: return false
    at(y - 1, x) in VLine or at(y + 1, x) in VLine
  proc ox(x: int): float = 10 + (x.float + 0.5) * CellW
  proc oy(y: int): float = 10 + (y.float + 0.5) * CellH
  var used = newSeq[seq[bool]](grid.len)
  for y in 0 ..< grid.len: used[y] = newSeq[bool](cols)
  # boîtes : du coin « + » supérieur gauche, la plus petite fermée à droite puis en bas
  type BoxCell = tuple[x0, y0, x1, y1: int]
  var boxes: seq[BoxCell]
  for y in 0 ..< grid.len:
    for x in 0 ..< grid[y].len:
      if at(y, x) notin {'+', '/'}: continue
      var x1 = x + 1
      while at(y, x1) in {'-', '='}: inc x1
      if x1 == x + 1 or at(y, x1) notin {'+', '\\'}: continue
      var y1 = y + 1
      while at(y1, x) in {'|', ':'}: inc y1
      if y1 == y + 1 or at(y1, x) notin {'+', '\\'}: continue
      var closed = at(y1, x1) in {'+', '/'}
      for k in x + 1 ..< x1:
        if at(y1, k) notin {'-', '=', '+'}: closed = false
      for k in y + 1 ..< y1:
        if at(k, x1) notin {'|', ':', '+'}: closed = false
      if closed: boxes.add (x, y, x1, y1)
  # fonds d'abord, puis texte : les couleurs « cXXX » désignent la boîte qui les contient
  for b in boxes:
    var fill = Fill
    for y in b.y0 + 1 ..< b.y1:
      for x in b.x0 + 1 ..< b.x1 - 3:
        if at(y, x) == 'c' and at(y, x - 1) in {' ', '|', ':'}:
          let token = $grid[y][x + 1] & $grid[y][x + 2] & $grid[y][x + 3]
          let color = colorTag(token)
          if color.len > 0:
            fill = color
            for k in 0 .. 3: used[y][x + k] = true
    svg.rect(ox(b.x0), oy(b.y0), ox(b.x1) - ox(b.x0), oy(b.y1) - oy(b.y0), fill, "none", 0, 0)
    ctx.region(nums[b.y0], ox(b.x0), oy(b.y0), ox(b.x1) - ox(b.x0), oy(b.y1) - oy(b.y0))
  # traits : chaque case relie son centre à ses voisines
  for y in 0 ..< grid.len:
    for x in 0 ..< grid[y].len:
      let c = at(y, x)
      let horizontal = isH(y, x)
      let vertical = isV(y, x)
      let dashH = if c == '=': "4 3" else: ""
      let dashV = if c == ':': "4 3" else: ""
      if horizontal or vertical or (c in {'/', '\\'} and (at(y, x - 1) in HLine or at(y, x + 1) in HLine)):
        used[y][x] = true
      if horizontal or c in {'/', '\\'}:
        if at(y, x + 1) in HLine and isH(y, x + 1) or at(y, x + 1) in {'/', '\\', '>'}:
          svg.line(ox(x), oy(y), ox(x + 1), oy(y), Stroke, LineWidth, dashH)
      if vertical or c in {'/', '\\'}:
        if at(y + 1, x) in VLine and isV(y + 1, x) or at(y + 1, x) in {'/', '\\', 'v', 'V'}:
          svg.line(ox(x), oy(y), ox(x), oy(y + 1), Stroke, LineWidth, dashV)
      # pointes
      if c == '>' and at(y, x - 1) in {'-', '=', '+'}:
        svg.polygon([(ox(x) + 3, oy(y)), (ox(x) - 4, oy(y) - 4), (ox(x) - 4, oy(y) + 4)], Stroke, Stroke)
        used[y][x] = true
      elif c == '<' and at(y, x + 1) in {'-', '=', '+'}:
        svg.line(ox(x), oy(y), ox(x + 1), oy(y))
        svg.polygon([(ox(x) - 3, oy(y)), (ox(x) + 4, oy(y) - 4), (ox(x) + 4, oy(y) + 4)], Stroke, Stroke)
        used[y][x] = true
      elif c == '^' and at(y + 1, x) in {'|', ':', '+'}:
        svg.line(ox(x), oy(y), ox(x), oy(y + 1))
        svg.polygon([(ox(x), oy(y) - 4), (ox(x) - 4, oy(y) + 3), (ox(x) + 4, oy(y) + 3)], Stroke, Stroke)
        used[y][x] = true
      elif c in {'v', 'V'} and at(y - 1, x) in {'|', ':', '+'}:
        svg.polygon([(ox(x), oy(y) + 4), (ox(x) - 4, oy(y) - 3), (ox(x) + 4, oy(y) - 3)], Stroke, Stroke)
        used[y][x] = true
  # texte restant, par suites de caractères
  for y in 0 ..< grid.len:
    var x = 0
    while x < grid[y].len:
      if used[y][x] or at(y, x) == ' ':
        inc x
        continue
      var e = x
      var word = ""
      while e < grid[y].len and not used[y][e] and
          (at(y, e) != ' ' or (e + 1 < grid[y].len and at(y, e + 1) != ' ' and not used[y][e + 1])):
        word.add $grid[y][e]
        inc e
      let shown = word.replace("{d}", "").replace("{s}", "").replace("{io}", "").replace("{o}", "").replace("{c}", "").replace("{mo}", "").strip()
      if shown.len > 0:
        svg.text(10 + x.float * CellW, oy(y) + 4, shown, size = 13, mono = true)
        ctx.region(nums[y], 10 + x.float * CellW, oy(y) - CellH / 2, shown.runeLen.float * CellW, CellH)
      x = e
  (svg, 20 + cols.float * CellW, 20 + grid.len.float * CellH)
