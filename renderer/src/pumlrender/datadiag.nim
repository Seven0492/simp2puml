## Données structurées : @startjson et @startyaml (sous-ensemble YAML courant).
import std/[strutils, json]
import common, preprocess

type DBox = ref object
  node: JsonNode
  rows: seq[(string, string, DBox)]  # clé, valeur scalaire, enfant
  isArray: bool
  keyW, valW, w, h, x, y, sub: float

const RowH = 20.0

proc scalar(n: JsonNode): string =
  case n.kind
  of JString: n.getStr
  of JInt: $n.getInt
  of JFloat: $n.getFloat
  of JBool: $n.getBool
  of JNull: "null"
  else: ""

proc build(n: JsonNode): DBox =
  result = DBox(node: n, isArray: n.kind == JArray)
  if n.kind == JObject:
    for k, v in n.pairs:
      if v.kind in {JObject, JArray}: result.rows.add (k, "", build(v))
      else: result.rows.add (k, scalar(v), nil)
  elif n.kind == JArray:
    for v in n.items:
      if v.kind in {JObject, JArray}: result.rows.add ("", "", build(v))
      else: result.rows.add ("", scalar(v), nil)
  else:
    result.rows.add ("", scalar(n), nil)
  for r in result.rows:
    result.keyW = max(result.keyW, textWidth(r[0], 12, true))
    result.valW = max(result.valW, textWidth(r[1], 12))
  if result.keyW > 0: result.keyW += 16
  result.valW = max(result.valW + 16, 24)
  result.w = result.keyW + result.valW
  result.h = max(1, result.rows.len).float * RowH

proc layout(b: DBox) =
  var s = 0.0
  var any = false
  for r in b.rows:
    if r[2] != nil:
      layout(r[2])
      if any: s += 10
      s += r[2].sub
      any = true
  b.sub = max(b.h, s)

proc place(b: DBox, x, y: float) =
  b.x = x
  b.y = y
  var yy = y
  for r in b.rows:
    if r[2] != nil:
      place(r[2], x + b.w + 50, yy)
      yy += r[2].sub + 10

proc draw(svg: var Svg, b: DBox, line: int, ctx: Context, maxX, maxY: var float) =
  maxX = max(maxX, b.x + b.w)
  maxY = max(maxY, b.y + b.h)
  svg.rect(b.x, b.y, b.w, b.h, Fill, Stroke, 4)
  for i, r in b.rows:
    let ry = b.y + float(i) * RowH
    if i > 0: svg.line(b.x, ry, b.x + b.w, ry, "#C7CEDB", 1)
    if b.keyW > 0:
      svg.rect(b.x, ry, b.keyW, RowH, HeaderFill, "none", 0, 0)
      svg.text(b.x + 8, ry + 14, r[0], size = 12, bold = true)
    let vx = b.x + b.keyW
    if r[2] == nil:
      svg.text(vx + 8, ry + 14, r[1], size = 12,
        fill = (if r[1] in ["true", "false", "null"]: Accent else: Ink))
    else:
      let c = r[2]
      let sx = b.x + b.w
      let sy = ry + RowH / 2
      let ex = c.x
      let ey = c.y + RowH / 2
      let mx = (sx + ex) / 2
      svg.circle(sx - 8, sy, 3, Stroke, Stroke)
      svg.path("M" & fmtF(sx - 8) & " " & fmtF(sy) & " C" & fmtF(mx) & " " & fmtF(sy) & " " & fmtF(mx) & " " &
        fmtF(ey) & " " & fmtF(ex) & " " & fmtF(ey))
      draw(svg, c, line, ctx, maxX, maxY)
  if b.keyW > 0: svg.line(b.x + b.keyW, b.y, b.x + b.keyW, b.y + b.h, "#C7CEDB", 1)
  svg.rect(b.x, b.y, b.w, b.h, "none", Stroke, 4)
  ctx.region(line, b.x, b.y, b.w, b.h)

# ------------------------------------------------------------------ YAML

proc yScalar(s: string): JsonNode =
  var t = s.strip()
  let hash = t.find(" #")
  if hash >= 0 and not (t.startsWith("\"") or t.startsWith("'")): t = t[0 ..< hash].strip()
  if t.len >= 2 and ((t[0] == '"' and t[^1] == '"') or (t[0] == '\'' and t[^1] == '\'')):
    return newJString(t[1 .. ^2])
  if t in ["true", "True", "yes"]: return newJBool(true)
  if t in ["false", "False", "no"]: return newJBool(false)
  if t in ["null", "~", ""]: return newJNull()
  try: return newJInt(parseInt(t)) except ValueError: discard
  try: return newJFloat(parseFloat(t)) except ValueError: discard
  if t.startsWith("[") and t.endsWith("]"):
    result = newJArray()
    for p in t[1 .. ^2].split(','):
      if p.strip().len > 0: result.add yScalar(p)
    return
  newJString(t)

type YLine = object
  indent: int
  text: string
  num: int

proc indentOf(s: string): int =
  while result < s.len and s[result] == ' ': inc result

proc parseBlock(ls: seq[YLine], i: var int, indent: int, ctx: Context): JsonNode

proc parseValueAfter(ls: seq[YLine], i: var int, parentIndent: int, inline: string, ctx: Context): JsonNode =
  if inline.strip().len > 0 and inline.strip() notin ["|", ">", "|-", ">-"]:
    return yScalar(inline)
  if inline.strip() in ["|", ">", "|-", ">-"]:
    var parts: seq[string]
    while i < ls.len and ls[i].indent > parentIndent:
      parts.add ls[i].text
      inc i
    return newJString(parts.join(if inline.strip().startsWith("|"): "\n" else: " "))
  if i < ls.len and (ls[i].indent > parentIndent or (ls[i].indent == parentIndent and ls[i].text.startsWith("- "))):
    return parseBlock(ls, i, ls[i].indent, ctx)
  newJNull()

proc splitKey(t: string): (string, string, bool) =
  var quote = '\0'
  for k, c in t:
    if quote != '\0':
      if c == quote: quote = '\0'
    elif c in {'"', '\''}: quote = c
    elif c == ':' and (k + 1 == t.len or t[k + 1] == ' '):
      return (t[0 ..< k].strip().strip(chars = {'"', '\''}), t[k + 1 .. ^1], true)
  ("", t, false)

proc parseBlock(ls: seq[YLine], i: var int, indent: int, ctx: Context): JsonNode =
  if i >= ls.len: return newJNull()
  if ls[i].text.startsWith("- ") or ls[i].text == "-":
    result = newJArray()
    while i < ls.len and ls[i].indent == indent and (ls[i].text.startsWith("- ") or ls[i].text == "-"):
      let item = if ls[i].text == "-": "" else: ls[i].text[2 .. ^1]
      let (k, v, isMap) = splitKey(item)
      let num = ls[i].num
      inc i
      if isMap:
        # élément de liste de type objet : la suite est indentée de indent + 2
        var obj = newJObject()
        obj[k] = parseValueAfter(ls, i, indent + 2, v, ctx)
        while i < ls.len and ls[i].indent == indent + 2 and not ls[i].text.startsWith("- "):
          let (k2, v2, ok2) = splitKey(ls[i].text)
          if not ok2:
            ctx.warn(ls[i].num, "ligne YAML ignorée : " & ls[i].text)
            inc i
            continue
          inc i
          obj[k2] = parseValueAfter(ls, i, indent + 2, v2, ctx)
        result.add obj
      elif item.strip().len == 0:
        result.add parseValueAfter(ls, i, indent, "", ctx)
      else:
        result.add yScalar(item)
      discard num
    return
  result = newJObject()
  while i < ls.len and ls[i].indent == indent:
    let (k, v, ok) = splitKey(ls[i].text)
    if not ok:
      ctx.warn(ls[i].num, "ligne YAML ignorée : " & ls[i].text)
      inc i
      continue
    inc i
    result[k] = parseValueAfter(ls, i, indent, v, ctx)
  if i < ls.len and ls[i].indent > indent:
    ctx.warn(ls[i].num, "indentation YAML inattendue")

proc parseYaml(doc: Doc, ctx: Context): JsonNode =
  var ls: seq[YLine]
  for l in doc.lines:
    let raw = l.text.replace("\t", "  ")
    let t = raw.strip()
    if t.len == 0 or t.startsWith("#") or t == "---": continue
    ls.add YLine(indent: indentOf(raw), text: t, num: l.num)
  if ls.len == 0: return newJNull()
  var i = 0
  result = parseBlock(ls, i, ls[0].indent, ctx)
  if i < ls.len: ctx.warn(ls[i].num, "fin du document YAML non interprétée")

proc renderData*(doc: Doc, ctx: Context): (Svg, float, float) =
  var svg: Svg
  var node: JsonNode
  if doc.kind == dkJson:
    var text = ""
    for l in doc.lines: text.add l.text & "\n"
    if text.strip().len == 0: return (svg, 0.0, 0.0)
    try:
      node = parseJson(text)
    except CatchableError as e:
      var line = doc.startLine
      let msg = e.msg
      # parseJson indique « input(l, c) » : on récupère la ligne relative
      let p = msg.find('(')
      if p >= 0:
        let comma = msg.find(',', p)
        if comma > p:
          try: line = doc.startLine + parseInt(msg[p + 1 ..< comma]) except ValueError: discard
      ctx.error(line, "JSON invalide : " & msg)
      return (svg, 0.0, 0.0)
  else:
    node = parseYaml(doc, ctx)
    if node.kind == JNull: return (svg, 0.0, 0.0)
  let b = build(node)
  layout(b)
  place(b, 20, 20)
  var maxX, maxY = 0.0
  draw(svg, b, doc.startLine + 1, ctx, maxX, maxY)
  (svg, maxX + 20, maxY + 20)
