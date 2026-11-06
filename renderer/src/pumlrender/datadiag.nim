## Données structurées : @startjson et @startyaml (sous-ensemble YAML courant).
## Chaque valeur garde sa ligne d'origine : un clic dans l'aperçu ramène à la
## bonne ligne du texte.
import std/[strutils, parsejson, streams]
import common, preprocess

type
  DKind = enum dScalar, dObject, dArray
  DNode = ref object
    kind: DKind
    value: string
    line: int
    items: seq[(string, DNode)]  # clé (vide dans un tableau), valeur
  Row = object
    key, value: string
    child: DBox
    line: int
  DBox = ref object
    rows: seq[Row]
    line: int
    keyW, valW, w, h, x, y, sub: float

const RowH = 20.0

proc build(n: DNode): DBox =
  result = DBox(line: n.line)
  if n.kind == dScalar:
    result.rows.add Row(value: n.value, line: n.line)
  else:
    for (k, v) in n.items:
      if v.kind == dScalar: result.rows.add Row(key: k, value: v.value, line: v.line)
      else: result.rows.add Row(key: k, child: build(v), line: v.line)
  for r in result.rows:
    result.keyW = max(result.keyW, textWidth(r.key, 12, true))
    result.valW = max(result.valW, textWidth(r.value, 12))
  if result.keyW > 0: result.keyW += 16
  result.valW = max(result.valW + 16, 24)
  result.w = result.keyW + result.valW
  result.h = max(1, result.rows.len).float * RowH

proc layout(b: DBox) =
  var s = 0.0
  var any = false
  for r in b.rows:
    if r.child != nil:
      layout(r.child)
      if any: s += 10
      s += r.child.sub
      any = true
  b.sub = max(b.h, s)

proc place(b: DBox, x, y: float) =
  b.x = x
  b.y = y
  var yy = y
  for r in b.rows:
    if r.child != nil:
      place(r.child, x + b.w + 50, yy)
      yy += r.child.sub + 10

proc draw(svg: var Svg, b: DBox, ctx: Context, maxX, maxY: var float) =
  maxX = max(maxX, b.x + b.w)
  maxY = max(maxY, b.y + b.h)
  svg.rect(b.x, b.y, b.w, b.h, Fill, Stroke, 4)
  if b.rows.len == 0: ctx.region(b.line, b.x, b.y, b.w, b.h)
  for i, r in b.rows:
    let ry = b.y + float(i) * RowH
    if i > 0: svg.line(b.x, ry, b.x + b.w, ry, Muted, 1)
    if b.keyW > 0:
      svg.rect(b.x, ry, b.keyW, RowH, HeaderFill, "none", 0, 0)
      svg.text(b.x + 8, ry + 14, r.key, size = 12, bold = true)
    let vx = b.x + b.keyW
    ctx.region(r.line, b.x, ry, b.w, RowH)
    if r.child == nil:
      svg.text(vx + 8, ry + 14, r.value, size = 12,
        fill = (if r.value in ["true", "false", "null"]: Accent else: Ink))
    else:
      let c = r.child
      let sx = b.x + b.w
      let sy = ry + RowH / 2
      let ex = c.x
      let ey = c.y + RowH / 2
      let mx = (sx + ex) / 2
      svg.beginLink(ctx, c.line)
      svg.circle(sx - 8, sy, 3, Stroke, Stroke)
      svg.path("M" & fmtF(sx - 8) & " " & fmtF(sy) & " C" & fmtF(mx) & " " & fmtF(sy) & " " & fmtF(mx) & " " &
        fmtF(ey) & " " & fmtF(ex) & " " & fmtF(ey))
      ctx.linkCubic(c.line, (sx - 8, sy), (mx, sy), (mx, ey), (ex, ey))
      svg.endLink(c.line)
      draw(svg, c, ctx, maxX, maxY)
  if b.keyW > 0: svg.line(b.x + b.keyW, b.y, b.x + b.keyW, b.y + b.h, Muted, 1)
  svg.rect(b.x, b.y, b.w, b.h, "none", Stroke, 4)

# ------------------------------------------------------------------ JSON

type JsonLineError = object of CatchableError
  line: int  ## ligne dans le texte du bloc (à partir de 1)

proc fail(p: JsonParser) {.noreturn.} =
  # errorMsg n'est valable que sur un évènement d'erreur
  let message = if p.kind == jsonError: errorMsg(p) else: "input(" & $getLine(p) & ", " & $getColumn(p) & ") contenu inattendu"
  var e = newException(JsonLineError, message)
  e.line = getLine(p)
  raise e

proc readJson(p: var JsonParser, doc: Doc, depth: int): DNode =
  ## `p` est placé sur le premier évènement de la valeur.
  template docLine(): int =
    let k = getLine(p) - 1
    if k >= 0 and k < doc.lines.len: doc.lines[k].num else: doc.startLine
  if depth > MaxNesting: tooDeep(docLine())
  case p.kind
  of jsonString, jsonInt, jsonFloat:
    result = DNode(kind: dScalar, value: p.str, line: docLine())
  of jsonTrue: result = DNode(kind: dScalar, value: "true", line: docLine())
  of jsonFalse: result = DNode(kind: dScalar, value: "false", line: docLine())
  of jsonNull: result = DNode(kind: dScalar, value: "null", line: docLine())
  of jsonObjectStart:
    result = DNode(kind: dObject, line: docLine())
    next(p)
    while p.kind != jsonObjectEnd:
      if p.kind != jsonString: fail(p)
      let key = p.str
      next(p)
      result.items.add (key, readJson(p, doc, depth + 1))
      next(p)
  of jsonArrayStart:
    result = DNode(kind: dArray, line: docLine())
    next(p)
    while p.kind != jsonArrayEnd:
      result.items.add ("", readJson(p, doc, depth + 1))
      next(p)
  else: fail(p)

proc parseJsonDoc(text: string, doc: Doc): DNode =
  var p: JsonParser
  open(p, newStringStream(text), "input")
  defer: close(p)
  next(p)
  result = readJson(p, doc, 0)
  next(p)
  if p.kind != jsonEof: fail(p)

# ------------------------------------------------------------------ YAML

proc splitFlow(s: string): seq[string] =
  ## Découpe « a, [b, c], {d: e} » aux virgules de premier niveau.
  var depth = 0
  var quote = '\0'
  var start = 0
  for k, c in s:
    if quote != '\0':
      if c == quote: quote = '\0'
    elif c in {'"', '\''}: quote = c
    elif c in {'[', '{'}: inc depth
    elif c in {']', '}'}: dec depth
    elif c == ',' and depth == 0:
      result.add s[start ..< k]
      start = k + 1
  result.add s[start .. ^1]

proc splitKey(t: string): (string, string, bool)

proc yScalar(s: string, line: int, depth = 0): DNode =
  if depth > MaxNesting: tooDeep(line)
  var t = s.strip()
  let hash = t.find(" #")
  if hash >= 0 and not (t.startsWith("\"") or t.startsWith("'")): t = t[0 ..< hash].strip()
  if t.len >= 2 and ((t[0] == '"' and t[^1] == '"') or (t[0] == '\'' and t[^1] == '\'')):
    return DNode(kind: dScalar, value: t[1 .. ^2], line: line)
  if t in ["true", "True", "yes"]: return DNode(kind: dScalar, value: "true", line: line)
  if t in ["false", "False", "no"]: return DNode(kind: dScalar, value: "false", line: line)
  if t in ["null", "~", ""]: return DNode(kind: dScalar, value: "null", line: line)
  if t.startsWith("[") and t.endsWith("]"):
    result = DNode(kind: dArray, line: line)
    for p in splitFlow(t[1 .. ^2]):
      if p.strip().len > 0: result.items.add ("", yScalar(p, line, depth + 1))
    return
  if t.startsWith("{") and t.endsWith("}"):
    result = DNode(kind: dObject, line: line)
    for p in splitFlow(t[1 .. ^2]):
      let (k, v, ok) = splitKey(p.strip())
      if ok: result.items.add (k, yScalar(v, line, depth + 1))
    return
  DNode(kind: dScalar, value: t, line: line)

type YLine = object
  indent: int
  text: string
  num: int

proc indentOf(s: string): int =
  while result < s.len and s[result] == ' ': inc result

proc parseBlock(ls: seq[YLine], i: var int, indent: int, ctx: Context, depth: int): DNode

proc parseValueAfter(ls: seq[YLine], i: var int, parentIndent: int, inline: string, line: int, ctx: Context, depth: int): DNode =
  if inline.strip().len > 0 and inline.strip() notin ["|", ">", "|-", ">-"]:
    return yScalar(inline, line)
  if inline.strip() in ["|", ">", "|-", ">-"]:
    var parts: seq[string]
    while i < ls.len and ls[i].indent > parentIndent:
      parts.add ls[i].text
      inc i
    return DNode(kind: dScalar, value: parts.join(if inline.strip().startsWith("|"): "\n" else: " "), line: line)
  if i < ls.len and (ls[i].indent > parentIndent or (ls[i].indent == parentIndent and ls[i].text.startsWith("- "))):
    return parseBlock(ls, i, ls[i].indent, ctx, depth)
  DNode(kind: dScalar, value: "null", line: line)

proc splitKey(t: string): (string, string, bool) =
  var quote = '\0'
  for k, c in t:
    if quote != '\0':
      if c == quote: quote = '\0'
    elif c in {'"', '\''}: quote = c
    elif c == ':' and (k + 1 == t.len or t[k + 1] == ' '):
      return (t[0 ..< k].strip().strip(chars = {'"', '\''}), t[k + 1 .. ^1], true)
  ("", t, false)

proc parseBlock(ls: seq[YLine], i: var int, indent: int, ctx: Context, depth: int): DNode =
  if i >= ls.len: return DNode(kind: dScalar, value: "null", line: 0)
  if depth > MaxNesting: tooDeep(ls[i].num)
  let first = ls[i].num
  if ls[i].text.startsWith("- ") or ls[i].text == "-":
    result = DNode(kind: dArray, line: first)
    while i < ls.len and ls[i].indent == indent and (ls[i].text.startsWith("- ") or ls[i].text == "-"):
      let item = if ls[i].text == "-": "" else: ls[i].text[2 .. ^1]
      let (k, v, isMap) = splitKey(item)
      let num = ls[i].num
      inc i
      if isMap:
        # élément de liste de type objet : la suite est indentée de indent + 2
        var obj = DNode(kind: dObject, line: num)
        let first = parseValueAfter(ls, i, indent + 2, v, num, ctx, depth + 1)
        first.line = num
        obj.items.add (k, first)
        while i < ls.len and ls[i].indent == indent + 2 and not ls[i].text.startsWith("- "):
          let (k2, v2, ok2) = splitKey(ls[i].text)
          let num2 = ls[i].num
          if not ok2:
            ctx.warn(num2, "ligne YAML ignorée : " & ls[i].text)
            inc i
            continue
          inc i
          let value = parseValueAfter(ls, i, indent + 2, v2, num2, ctx, depth + 1)
          value.line = num2  # la ligne de la clé, pas celle de son premier enfant
          obj.items.add (k2, value)
        result.items.add ("", obj)
      elif item.strip().len == 0:
        result.items.add ("", parseValueAfter(ls, i, indent, "", num, ctx, depth + 1))
      else:
        result.items.add ("", yScalar(item, num))
    return
  result = DNode(kind: dObject, line: first)
  while i < ls.len and ls[i].indent == indent:
    let (k, v, ok) = splitKey(ls[i].text)
    let num = ls[i].num
    if not ok:
      ctx.warn(num, "ligne YAML ignorée : " & ls[i].text)
      inc i
      continue
    inc i
    let value = parseValueAfter(ls, i, indent, v, num, ctx, depth + 1)
    value.line = num  # la ligne de la clé, pas celle de son premier enfant
    result.items.add (k, value)
  if i < ls.len and ls[i].indent > indent:
    ctx.warn(ls[i].num, "indentation YAML inattendue")

proc parseYaml(doc: Doc, ctx: Context): DNode =
  var ls: seq[YLine]
  for l in doc.lines:
    let raw = l.text.replace("\t", "  ")
    let t = raw.strip()
    if t.len == 0 or t.startsWith("#") or t == "---": continue
    ls.add YLine(indent: indentOf(raw), text: t, num: l.num)
  if ls.len == 0: return nil
  var i = 0
  result = parseBlock(ls, i, ls[0].indent, ctx, 0)
  if i < ls.len: ctx.warn(ls[i].num, "fin du document YAML non interprétée")

proc renderData*(doc: Doc, ctx: Context): (Svg, float, float) =
  var svg: Svg
  var node: DNode
  if doc.kind == dkJson:
    var text = ""
    for l in doc.lines: text.add l.text & "\n"
    if text.strip().len == 0: return (svg, 0.0, 0.0)
    try:
      node = parseJsonDoc(text, doc)
    except JsonLineError as e:
      let k = e.line - 1
      let line = if k >= 0 and k < doc.lines.len: doc.lines[k].num else: doc.startLine
      ctx.error(line, "JSON invalide : " & e.msg)
      return (svg, 0.0, 0.0)
  else:
    node = parseYaml(doc, ctx)
    if node == nil: return (svg, 0.0, 0.0)
  let b = build(node)
  layout(b)
  place(b, 20, 20)
  var maxX, maxY = 0.0
  draw(svg, b, ctx, maxX, maxY)
  (svg, maxX + 20, maxY + 20)
