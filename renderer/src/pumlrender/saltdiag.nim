## Maquettes d'écran @startsalt : grilles, boutons, champs, cases, listes, arbres, onglets.
import std/[strutils]
import common, preprocess

type
  CellKind = enum ckText, ckButton, ckInput, ckCheck, ckRadio, ckCombo, ckGrid, ckEmpty, ckSpan, ckSeparator
  Cell = ref object
    kind: CellKind
    text: string
    on: bool
    grid: Grid
    line: int
    w, h: float
  GridStyle = enum gsNone, gsOuter, gsAll, gsVertical, gsHorizontal, gsTree, gsTabs, gsMenu
  Grid = ref object
    style: GridStyle
    rows: seq[seq[Cell]]
    colW, rowH: seq[float]
    w, h: float

const
  Pad = 4.0
  RowMin = 22.0

proc parseCell(raw: string, line: int): Cell =
  let t = raw.strip()
  result = Cell(kind: ckText, text: t, line: line)
  if t == "." or t.len == 0: result.kind = ckEmpty; return
  if t == "*": result.kind = ckSpan; return
  if t in ["..", "--", "==", "~~"]: (result.kind = ckSeparator; return)
  if t.startsWith("[X]") or t.startsWith("[x]") or t.startsWith("[]"):
    result.kind = ckCheck
    result.on = not t.startsWith("[]")
    result.text = t[(if t.startsWith("[]"): 2 else: 3) .. ^1].strip()
  elif t.startsWith("(X)") or t.startsWith("(x)") or t.startsWith("()"):
    result.kind = ckRadio
    result.on = not t.startsWith("()")
    result.text = t[(if t.startsWith("()"): 2 else: 3) .. ^1].strip()
  elif t.len >= 2 and t[0] == '[' and t[^1] == ']':
    result.kind = ckButton
    result.text = t[1 .. ^2].strip()
  elif t.len >= 2 and t[0] == '"' and t[^1] == '"':
    result.kind = ckInput
    result.text = t[1 .. ^2]
  elif t.len >= 2 and t[0] == '^' and t[^1] == '^':
    result.kind = ckCombo
    result.text = t[1 .. ^2].split('^')[0].strip()

proc splitCells(s: string): seq[string] =
  ## Découpe aux « | » hors de "…", […] et ^…^.
  var inQuote = false
  var inBracket = 0
  var cur = ""
  for c in s:
    case c
    of '"': inQuote = not inQuote
    of '[': inc inBracket
    of ']': dec inBracket
    else: discard
    if c == '|' and not inQuote and inBracket <= 0:
      result.add cur
      cur = ""
    else:
      cur.add c
  result.add cur

proc parseGrid(lines: seq[SrcLine], i: var int, header: string, ctx: Context, depth: int): Grid =
  if depth > MaxNesting: tooDeep(lines[min(i, lines.high)].num)
  result = Grid()
  case header.strip()
  of "{+": result.style = gsOuter
  of "{#": result.style = gsAll
  of "{!": result.style = gsVertical
  of "{-": result.style = gsHorizontal
  of "{T", "{T+", "{T#", "{T!", "{T-": result.style = gsTree
  of "{/": result.style = gsTabs
  of "{*": result.style = gsMenu
  of "{", "{S", "{SI", "{S-", "{^": result.style = gsNone
  else: result.style = gsNone
  while i < lines.len:
    let l = lines[i]
    let t = l.text.strip()
    inc i
    if t.len == 0: continue
    if t == "}": return
    var row: seq[Cell]
    if result.style == gsTree:
      # « ++ libellé » : un cran par « + », les colonnes suivantes séparées par « | »
      var depthMarks = 0
      while depthMarks < t.len and t[depthMarks] == '+': inc depthMarks
      let parts = splitCells(t[depthMarks .. ^1])
      let first = Cell(kind: ckText, text: repeat("    ", max(0, depthMarks - 1)) & (if depthMarks > 0: "▸ " else: "") & parts[0].strip(), line: l.num)
      row.add first
      for p in parts[1 .. ^1]: row.add parseCell(p, l.num)
      result.rows.add row
      continue
    # une ligne peut se terminer par l'ouverture d'une grille imbriquée
    var parts = splitCells(t)
    for k, p in parts:
      let c = p.strip()
      if c.startsWith("{") and not c.contains('}'):
        row.add Cell(kind: ckGrid, grid: parseGrid(lines, i, c, ctx, depth + 1), line: l.num)
      elif c == "}":
        result.rows.add row
        return
      else:
        row.add parseCell(p, l.num)
    result.rows.add row
  ctx.error(lines[max(0, min(i, lines.high))].num, "grille salt non fermée (« } » manquant)")

proc measure(g: Grid) =
  var cols = 0
  for r in g.rows: cols = max(cols, r.len)
  g.colW = newSeq[float](cols)
  g.rowH = newSeq[float](g.rows.len)
  for ri, r in g.rows:
    g.rowH[ri] = RowMin
    for ci, c in r:
      case c.kind
      of ckGrid:
        measure(c.grid)
        (c.w, c.h) = (c.grid.w + 2 * Pad, c.grid.h + 2 * Pad)
      of ckButton: (c.w, c.h) = (textWidth(c.text, 12) + 22, RowMin)
      of ckInput: (c.w, c.h) = (max(textWidth(c.text, 12) + 16, 60.0), RowMin)
      of ckCheck, ckRadio: (c.w, c.h) = (textWidth(c.text, 12) + 26, RowMin)
      of ckCombo: (c.w, c.h) = (textWidth(c.text, 12) + 34, RowMin)
      of ckText: (c.w, c.h) = (textWidth(c.text, 12) + 2 * Pad, RowMin)
      of ckSeparator: (c.w, c.h) = (10.0, 10.0)
      of ckEmpty, ckSpan: (c.w, c.h) = (2 * Pad, RowMin)
      g.colW[ci] = max(g.colW[ci], c.w)
      g.rowH[ri] = max(g.rowH[ri], c.h)
  g.w = 0
  for c in g.colW: g.w += c + Pad
  g.h = 0
  for r in g.rowH: g.h += r
  if g.style == gsTabs: g.h += 2
  g.w = max(g.w, 10)
  g.h = max(g.h, 10)

proc draw(svg: var Svg, g: Grid, x, y: float, ctx: Context) =
  let bg = HeaderFill
  if g.style in {gsOuter, gsAll}: svg.rect(x, y, g.w, g.h, "none", Stroke, 0, 1)
  if g.style in {gsMenu, gsTabs}: svg.rect(x, y, g.w, g.h, bg, "none", 0, 0)
  var ry = y
  for ri, r in g.rows:
    let rh = g.rowH[ri]
    var cx = x
    for ci, c in r:
      var cw = g.colW[ci] + Pad
      # « * » : la cellule précédente s'étend sur cette colonne
      var span = ci + 1
      while span < r.len and r[span].kind == ckSpan:
        cw += g.colW[span] + Pad
        inc span
      let mid = ry + rh / 2
      case c.kind
      of ckText:
        svg.text(cx + Pad, mid + 4, c.text, size = 12)
      of ckButton:
        svg.rect(cx + Pad / 2, ry + 2, cw - Pad, rh - 4, Fill, Stroke, 4, 1)
        svg.text(cx + cw / 2, mid + 4, c.text, "middle", 12)
      of ckInput:
        svg.rect(cx + Pad / 2, ry + 2, cw - Pad, rh - 4, Background, Stroke, 0, 1)
        # contenu saisi : littéral, jamais du balisage (« **** » est un mot de passe masqué)
        svg.text(cx + Pad + 2, mid + 4, c.text.replace("*", "~*"), size = 12)
      of ckCheck:
        svg.rect(cx + Pad, mid - 6, 12, 12, Background, Stroke, 0, 1)
        if c.on: svg.path("M" & fmtF(cx + Pad + 2) & " " & fmtF(mid) & " L" & fmtF(cx + Pad + 5) & " " & fmtF(mid + 4) & " L" & fmtF(cx + Pad + 10) & " " & fmtF(mid - 4), "none", Stroke, 1.6)
        svg.text(cx + Pad + 17, mid + 4, c.text, size = 12)
      of ckRadio:
        svg.circle(cx + Pad + 6, mid, 6, Background, Stroke, 1)
        if c.on: svg.circle(cx + Pad + 6, mid, 3, Stroke, Stroke, 1)
        svg.text(cx + Pad + 17, mid + 4, c.text, size = 12)
      of ckCombo:
        svg.rect(cx + Pad / 2, ry + 2, cw - Pad, rh - 4, Background, Stroke, 0, 1)
        svg.text(cx + Pad + 2, mid + 4, c.text, size = 12)
        let ax = cx + cw - 12
        svg.polygon([(ax - 4, mid - 2), (ax + 4, mid - 2), (ax, mid + 3)], Stroke, Stroke, 1)
      of ckGrid:
        svg.draw(c.grid, cx + Pad, ry + Pad, ctx)
      of ckSeparator:
        let dash = if c.text == "..": "2 3" else: ""
        var total = 0.0
        for w in g.colW: total += w + Pad
        svg.line(x, mid, x + total, mid, (if c.text == "==": Stroke else: Muted), (if c.text == "==": 1.6 else: 1), dash)
      of ckEmpty, ckSpan: discard
      if c.kind notin {ckEmpty, ckSpan, ckSeparator, ckGrid}: ctx.region(c.line, cx, ry, cw, rh)
      if g.style == gsAll and ci > 0: svg.line(cx, ry, cx, ry + rh, Stroke, 1)
      if g.style == gsVertical and ci > 0: svg.line(cx, ry, cx, ry + rh, Stroke, 1)
      if g.style == gsTabs and c.kind != ckEmpty: svg.rect(cx, ry, cw, rh, "none", Stroke, 2, 1)
      cx += g.colW[ci] + Pad
    if g.style in {gsAll, gsHorizontal} and ri > 0: svg.line(x, ry, x + g.w, ry, Stroke, 1)
    ry += rh

proc renderSalt*(doc: Doc, ctx: Context): (Svg, float, float) =
  var svg: Svg
  var i = 0
  let lines = doc.lines
  while i < lines.len and lines[i].text.strip().len == 0: inc i
  if i >= lines.len: return (svg, 0.0, 0.0)
  let header = lines[i].text.strip()
  var g: Grid
  if header.startsWith("{"):
    inc i
    g = parseGrid(lines, i, header, ctx, 0)
  else:
    # sans accolades : une ligne par rangée, jusqu'à la fin du bloc
    g = parseGrid(lines & @[SrcLine(num: lines[^1].num, text: "}")], i, "{", ctx, 0)
  measure(g)
  svg.draw(g, 10, 10, ctx)
  (svg, g.w + 20, g.h + 20)
