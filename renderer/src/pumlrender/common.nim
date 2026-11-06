## Types partagés : diagnostics, régions cliquables, métriques de texte,
## construction SVG et petit analyseur lexical sans expressions régulières.
import std/[strutils, math, tables]
from std/unicode import Rune, runes, runeAt, runeLen, runeLenAt, validateUtf8
import fontmetrics

type
  Severity* = enum
    sevError = "error", sevWarning = "warning", sevInfo = "info"

  Diagnostic* = object
    line*: int
    severity*: Severity
    message*: string

  Region* = object
    ## Zone de l'aperçu rattachée à une ligne source (1-based).
    line*: int
    x*, y*, w*, h*: float

  SrcLine* = object
    num*: int
    text*: string

  LinkPath* = object
    ## Tracé d'une flèche (coordonnées du SVG) : cliquable sur sa longueur.
    line*: int
    after*: int   ## nombre de régions déjà dessinées : les suivantes passent au-dessus
    points*: seq[(float, float)]

  Context* = ref object
    diags*: seq[Diagnostic]
    regions*: seq[Region]
    links*: seq[LinkPath]

  Meta* = object
    title*, header*, footer*, caption*, legend*: string
    titleLine*, legendLine*, headerLine*, footerLine*, captionLine*: int
    leftToRight*: bool
    legendAlign*: string

  Svg* = object
    body*: string
    defs*: string

const
  MonoFamily* = "DejaVu Sans Mono, Liberation Mono, monospace"
  DefaultFontFamily = "DejaVu Sans, Liberation Sans, Arial, Helvetica, sans-serif"
  ## Au-delà, le rendu s'arrête avec un diagnostic : les passes de mise en page
  ## sont récursives et une pile épuisée tuerait le processus sans message.
  MaxNesting* = 200

type
  ElementStyle* = object
    ## Réglages d'un type d'élément (`skinparam`, `<style>`) ; vide : hérité.
    bg*, border*, font*: string
    thickness*: float

  Sprite* = object
    width*, height*, levels*: int
    pixels*: seq[int]  ## largeur × hauteur niveaux de gris, 0 : transparent

# Palette du rendu en cours : remise à zéro par `resetStyle`, puis modifiée par
# `!theme`, `skinparam` et `<style>`. Le moteur traite un rendu à la fois.
var
  FontFamily* = DefaultFontFamily
  FontSize* = 13.0
  LineHeight* = 16.0
  LineWidth* = 1.2
  Ink* = "#1F2430"
  Stroke* = "#3B4252"
  Fill* = "#F7F8FC"
  HeaderFill* = "#E8ECF6"
  NoteFill* = "#FFF6BF"
  NoteStroke* = "#B49B2F"
  Accent* = "#3A6EA5"
  Muted* = "#6B7280"
  FrameFill* = "#FBFBFD"
  Background* = "#FFFFFF"
  ArrowColor* = ""     ## vide : celle du trait
  Elements*: Table[string, ElementStyle]
  Sprites*: Table[string, Sprite]

proc resetStyle*() =
  FontFamily = DefaultFontFamily
  FontSize = 13.0
  LineHeight = 16.0
  LineWidth = 1.2
  Ink = "#1F2430"
  Stroke = "#3B4252"
  Fill = "#F7F8FC"
  HeaderFill = "#E8ECF6"
  NoteFill = "#FFF6BF"
  NoteStroke = "#B49B2F"
  Accent = "#3A6EA5"
  Muted = "#6B7280"
  FrameFill = "#FBFBFD"
  Background = "#FFFFFF"
  ArrowColor = ""
  Elements.clear()
  Sprites.clear()

template styled*(element: string, body: untyped) =
  ## Exécute `body` avec les couleurs réglées pour ce type d'élément.
  let savedStyle = (Ink, Stroke, Fill, HeaderFill, LineWidth)
  let es = Elements.getOrDefault(element)
  if es.bg.len > 0:
    Fill = es.bg
    HeaderFill = es.bg
  if es.border.len > 0: Stroke = es.border
  if es.font.len > 0: Ink = es.font
  if es.thickness > 0: LineWidth = es.thickness
  body
  (Ink, Stroke, Fill, HeaderFill, LineWidth) = savedStyle

proc arrowStroke*(): string =
  if ArrowColor.len > 0: ArrowColor else: Stroke

type TooComplex* = object of CatchableError
  ## Diagramme refusé : la ligne fautive accompagne le message.
  line*: int

proc tooDeep*(line: int) {.noreturn.} =
  var e = newException(TooComplex, "imbrication trop profonde (maximum : " & $MaxNesting & " niveaux)")
  e.line = line
  raise e

proc newContext*(): Context = Context()

proc error*(ctx: Context, line: int, msg: string) =
  ctx.diags.add Diagnostic(line: line, severity: sevError, message: msg)

proc warn*(ctx: Context, line: int, msg: string) =
  ctx.diags.add Diagnostic(line: line, severity: sevWarning, message: msg)

proc info*(ctx: Context, line: int, msg: string) =
  ctx.diags.add Diagnostic(line: line, severity: sevInfo, message: msg)

proc region*(ctx: Context, line: int, x, y, w, h: float) =
  if line > 0:
    ctx.regions.add Region(line: line, x: x, y: y, w: w, h: h)

proc link*(ctx: Context, line: int, points: openArray[(float, float)]) =
  if line > 0 and points.len >= 2:
    ctx.links.add LinkPath(line: line, after: ctx.regions.len, points: @points)

proc beginLink*(svg: var Svg, ctx: Context, line: int) =
  ## Regroupe les traits et la pointe d'une flèche : l'éditeur les recolore ensemble au survol.
  ## Le numéro est celui du `ctx.link` qui suit, un seul par groupe.
  if line > 0: svg.body.add "<g data-link=\"" & $ctx.links.len & "\">\n"

proc endLink*(svg: var Svg, line: int) =
  if line > 0: svg.body.add "</g>\n"

proc linkCubic*(ctx: Context, line: int, p0, c1, c2, p3: (float, float)) =
  ## Courbe de Bézier approchée par des segments.
  const Steps = 12
  var pts: seq[(float, float)]
  for i in 0 .. Steps:
    let t = i.float / Steps
    let u = 1 - t
    pts.add (u * u * u * p0[0] + 3 * u * u * t * c1[0] + 3 * u * t * t * c2[0] + t * t * t * p3[0],
             u * u * u * p0[1] + 3 * u * u * t * c1[1] + 3 * u * t * t * c2[1] + t * t * t * p3[1])
  ctx.link(line, pts)

proc hasErrors*(ctx: Context): bool =
  for d in ctx.diags:
    if d.severity == sevError: return true

# ---------------------------------------------------------------- texte

proc runeWidth(r: Rune, bold: bool): float =
  ## Largeur réelle dans la police du diagramme, sinon une estimation.
  let c = int(r)
  if c >= FirstRune and c <= LastRune:
    return (if bold: Bold[c - FirstRune] else: Regular[c - FirstRune]).float
  if c >= 0x1100 and (c <= 0x115F or (c >= 0x2E80 and c <= 0xA4CF) or
      (c >= 0xAC00 and c <= 0xD7A3) or (c >= 0xF900 and c <= 0xFAFF) or
      (c >= 0xFE30 and c <= 0xFE4F) or (c >= 0xFF00 and c <= 0xFF60) or
      (c >= 0x1F300 and c <= 0x1FAFF)):
    return 1.0
  0.7

proc plainWidth(s: string, size = FontSize, bold = false): float =
  for r in s.runes: result += runeWidth(r, bold)
  result *= size

proc colorValue*(s: string): string

# ---------------------------------------------------------------- texte enrichi

type Run = object
  ## Morceau de texte de même aspect, ou sprite si `sprite` n'est pas vide.
  text, color, sprite: string
  bold, italic, underline, mono: bool
  size, scale: float

proc spriteParts(tag: string): (string, float, string) =
  ## « $nom{scale=2,color=red} » → (nom, échelle, couleur)
  var name = tag
  var scale = 1.0
  var color = ""
  let open = tag.find('{')
  if open >= 0 and tag.endsWith("}"):
    name = tag[0 ..< open]
    for part in tag[open + 1 .. ^2].split(','):
      let kv = part.split('=')
      if kv.len == 2:
        case kv[0].strip().toLowerAscii()
        of "scale":
          try: scale = clamp(parseFloat(kv[1].strip()), 0.1, 8.0) except ValueError: discard
        of "color": color = kv[1].strip()
        else: discard
  (name.strip(chars = {'$', ' '}), scale, color)

const MarkupStart = {'*', '/', '_', '"', '~', '<'}

proc parseRuns(s: string, size: float, bold, italic, underline, mono: bool, color: string): seq[Run] =
  ## Créole courant : **gras**, //italique//, __souligné__, ""mono"", ~~souligné~~,
  ## <b> <i> <u> <color:x> <size:n> et sprites <$nom>. Un « ~ » annule le balisage suivant.
  var cur = Run(size: size, bold: bold, italic: italic, underline: underline, mono: mono, color: color)
  var stack: seq[Run]
  template flush() =
    if cur.text.len > 0:
      result.add cur
      cur.text = ""
  var i = 0
  while i < s.len:
    let c = s[i]
    if c notin MarkupStart:
      cur.text.add c
      inc i
      continue
    let two = if i + 1 < s.len: s[i .. i + 1] else: ""
    if c == '~' and i + 1 < s.len and s[i + 1] in {'*', '/', '_', '"', '<', '~'} and two != "~~":
      cur.text.add s[i + 1]
      i += 2
    elif two == "**":
      flush()
      cur.bold = not cur.bold
      i += 2
    elif two == "//" and (i == 0 or s[i - 1] != ':'):
      flush()
      cur.italic = not cur.italic
      i += 2
    elif two == "__" or two == "~~":
      flush()
      cur.underline = not cur.underline
      i += 2
    elif two == "\"\"":
      flush()
      cur.mono = not cur.mono
      i += 2
    elif c == '<':
      let e = s.find('>', i)
      let tag = if e > i: s[i + 1 ..< e] else: ""
      let lw = tag.toLowerAscii()
      if tag.startsWith("$") and Sprites.hasKey(spriteParts(tag)[0]):
        flush()
        let (name, scale, col) = spriteParts(tag)
        result.add Run(sprite: name, scale: scale, color: colorValue(col), size: cur.size)
      elif lw == "b" or lw == "i" or lw == "u" or lw.startsWith("color") or lw.startsWith("size") or lw == "s":
        flush()
        stack.add cur
        case lw[0]
        of 'b': cur.bold = true
        of 'i': cur.italic = true
        of 'u': cur.underline = true
        of 'c':
          let v = colorValue(tag[5 .. ^1].strip(chars = {':', ' ', '='}))
          if v.len > 0: cur.color = v
        of 's':
          if lw != "s":
            try: cur.size = clamp(parseFloat(tag[4 .. ^1].strip(chars = {':', ' ', '='})), 4.0, 72.0)
            except ValueError: discard
        else: discard
      elif lw.startsWith("/") and lw[1 .. ^1].strip() in ["b", "i", "u", "s", "color", "size"]:
        flush()
        if stack.len > 0: cur = stack.pop()
      else:
        cur.text.add c
        inc i
        continue
      i = e + 1
    else:
      cur.text.add c
      inc i
  flush()

proc hasMarkup(s: string): bool =
  for c in s:
    if c in MarkupStart: return true

proc spriteSize(r: Run, lineSize: float): (float, float) =
  ## Un sprite plus haut que la ligne est réduit : la hauteur des boîtes ne dépend que du texte.
  let sp = Sprites[r.sprite]
  let k = min(r.scale, lineSize * 1.2 / max(1, sp.height).float)
  (sp.width.float * k, sp.height.float * k)

proc runWidth(r: Run): float =
  if r.sprite.len > 0: spriteSize(r, r.size)[0] + 2
  elif r.mono: r.text.runeLen.float * MonoAdvance * r.size
  else: plainWidth(r.text, r.size, r.bold)

proc textWidth*(s: string, size = FontSize, bold = false): float =
  if not hasMarkup(s): return plainWidth(s, size, bold)
  for r in parseRuns(s, size, bold, false, false, false, ""): result += runWidth(r)

proc blockWidth*(lines: seq[string], size = FontSize, bold = false): float =
  for l in lines: result = max(result, textWidth(l, size, bold))

proc xmlEscape*(s: string): string =
  ## Texte sûr pour le SVG : caractères spéciaux échappés, caractères
  ## interdits en XML retirés, UTF-8 invalide remplacé par U+FFFD.
  result = newStringOfCap(s.len)
  var i = 0
  while i < s.len:
    let c = s[i]
    if ord(c) < 0x80:
      case c
      of '&': result.add "&amp;"
      of '<': result.add "&lt;"
      of '>': result.add "&gt;"
      of '"': result.add "&quot;"
      of '\'': result.add "&#39;"
      elif ord(c) < 32 and c notin {'\t', '\n'}: discard
      else: result.add c
      inc i
      continue
    let n1 = runeLenAt(s, i)
    let chunk = s[i ..< min(s.len, i + n1)]
    if n1 < 2 or chunk.len != n1 or validateUtf8(chunk) != -1:
      result.add "\uFFFD"
      inc i
      continue
    let r = runeAt(s, i).int
    if r notin [0xFFFE, 0xFFFF] and r notin 0xD800 .. 0xDFFF:
      result.add chunk
    i += n1

proc fmtF*(f: float): string =
  ## Nombre compact pour SVG.
  if f.classify in {fcNan, fcInf, fcNegInf}: return "0"
  result = formatFloat(round(f * 100) / 100, ffDecimal, 2)
  result.trimZeros()
  if result == "-0": result = "0"

proc unescapeLabel*(s: string): string =
  ## Convertit les séquences PlantUML \\n et \\t ; le balisage créole reste, interprété par `text`.
  s.replace("\\n", "\n").replace("\\t", "    ").replace("%n()", "\n")

proc splitLabel*(s: string): seq[string] =
  result = unescapeLabel(s).split('\n')
  if result.len == 0: result = @[""]

proc stripQuotes*(s: string): string =
  let t = s.strip()
  if t.len >= 2 and t[0] == '"' and t[^1] == '"': t[1 .. ^2] else: t

proc colorValue*(s: string): string =
  ## #RRGGBB, #nom ou nom → couleur SVG sûre ; vide si invalide.
  var t = s.strip()
  if t.startsWith("#"): t = t[1 .. ^1]
  # dégradé « a/b », « a|b », « a\\b », « a-b » : la première couleur
  for sep in ['/', '|', '\\', '-']:
    let p = t.find(sep)
    if p > 0: t = t[0 ..< p]
  if t.len == 0: return ""
  case t.toLowerAscii()
  of "business": return "#FFFFCC"
  of "application": return "#C2F0FF"
  of "motivation": return "#CCCCFF"
  of "strategy": return "#F5DEAA"
  of "technology": return "#C9FFC9"
  of "physical": return "#97FF97"
  of "implementation": return "#FFE0E0"
  else: discard
  for c in t:
    if c notin {'0'..'9', 'a'..'z', 'A'..'Z'}: return ""
  var hex = t.len in [3, 6, 8]
  for c in t:
    if c notin HexDigits: hex = false
  if hex: "#" & t else: t.toLowerAscii()

# ---------------------------------------------------------------- SVG

proc add*(svg: var Svg, s: string) = svg.body.add s

proc rect*(svg: var Svg, x, y, w, h: float, fill = Fill, stroke = Stroke,
           rx = 0.0, sw = LineWidth, dash = "", extra = "") =
  svg.add "<rect x=\"" & fmtF(x) & "\" y=\"" & fmtF(y) & "\" width=\"" & fmtF(w) &
    "\" height=\"" & fmtF(h) & "\""
  if rx > 0: svg.add " rx=\"" & fmtF(rx) & "\""
  svg.add " fill=\"" & fill & "\" stroke=\"" & stroke & "\" stroke-width=\"" & fmtF(sw) & "\""
  if dash.len > 0: svg.add " stroke-dasharray=\"" & dash & "\""
  if extra.len > 0: svg.add " " & extra
  svg.add "/>\n"

proc line*(svg: var Svg, x1, y1, x2, y2: float, stroke = Stroke, sw = LineWidth, dash = "") =
  svg.add "<line x1=\"" & fmtF(x1) & "\" y1=\"" & fmtF(y1) & "\" x2=\"" & fmtF(x2) &
    "\" y2=\"" & fmtF(y2) & "\" stroke=\"" & stroke & "\" stroke-width=\"" & fmtF(sw) & "\""
  if dash.len > 0: svg.add " stroke-dasharray=\"" & dash & "\""
  svg.add "/>\n"

proc circle*(svg: var Svg, cx, cy, r: float, fill = Fill, stroke = Stroke, sw = LineWidth) =
  svg.add "<circle cx=\"" & fmtF(cx) & "\" cy=\"" & fmtF(cy) & "\" r=\"" & fmtF(r) &
    "\" fill=\"" & fill & "\" stroke=\"" & stroke & "\" stroke-width=\"" & fmtF(sw) & "\"/>\n"

proc ellipse*(svg: var Svg, cx, cy, rx, ry: float, fill = Fill, stroke = Stroke, sw = LineWidth) =
  svg.add "<ellipse cx=\"" & fmtF(cx) & "\" cy=\"" & fmtF(cy) & "\" rx=\"" & fmtF(rx) &
    "\" ry=\"" & fmtF(ry) & "\" fill=\"" & fill & "\" stroke=\"" & stroke &
    "\" stroke-width=\"" & fmtF(sw) & "\"/>\n"

proc path*(svg: var Svg, d: string, fill = "none", stroke = Stroke, sw = LineWidth, dash = "") =
  svg.add "<path d=\"" & d & "\" fill=\"" & fill & "\" stroke=\"" & stroke &
    "\" stroke-width=\"" & fmtF(sw) & "\" stroke-linejoin=\"round\""
  if dash.len > 0: svg.add " stroke-dasharray=\"" & dash & "\""
  svg.add "/>\n"

proc polygon*(svg: var Svg, pts: openArray[(float, float)], fill = Fill, stroke = Stroke, sw = LineWidth) =
  var s = ""
  for i, p in pts:
    if i > 0: s.add ' '
    s.add fmtF(p[0]) & "," & fmtF(p[1])
  svg.add "<polygon points=\"" & s & "\" fill=\"" & fill & "\" stroke=\"" & stroke &
    "\" stroke-width=\"" & fmtF(sw) & "\" stroke-linejoin=\"round\"/>\n"

proc plainText(svg: var Svg, x, y: float, s: string, anchor: string, size: float,
                bold, italic: bool, fill: string, underline, mono: bool) =
  svg.add "<text x=\"" & fmtF(x) & "\" y=\"" & fmtF(y) & "\" font-family=\"" &
    (if mono: MonoFamily else: FontFamily) & "\" font-size=\"" & fmtF(size) & "\" fill=\"" & fill & "\""
  if anchor != "start": svg.add " text-anchor=\"" & anchor & "\""
  if bold: svg.add " font-weight=\"bold\""
  if italic: svg.add " font-style=\"italic\""
  if underline: svg.add " text-decoration=\"underline\""
  svg.add ">" & xmlEscape(s) & "</text>\n"

proc drawSprite(svg: var Svg, r: Run, x, baseline: float) =
  ## Une rangée de rectangles par suite de pixels de même niveau.
  let sp = Sprites[r.sprite]
  let (w, h) = spriteSize(r, r.size)
  let k = w / max(1, sp.width).float
  let top = baseline - h + r.size * 0.2
  let col = if r.color.len > 0: r.color else: Ink
  for row in 0 ..< sp.height:
    var c = 0
    while c < sp.width:
      let v = sp.pixels[row * sp.width + c]
      var e = c + 1
      while e < sp.width and sp.pixels[row * sp.width + e] == v: inc e
      if v > 0:
        svg.add "<rect x=\"" & fmtF(x + c.float * k) & "\" y=\"" & fmtF(top + row.float * k) & "\" width=\"" &
          fmtF((e - c).float * k) & "\" height=\"" & fmtF(k) & "\" fill=\"" & col & "\" fill-opacity=\"" &
          fmtF(v.float / max(1, sp.levels - 1).float) & "\"/>\n"
      c = e

proc text*(svg: var Svg, x, y: float, s: string, anchor = "start", size = FontSize,
           bold = false, italic = false, fill = Ink, underline = false, mono = false) =
  ## y est la ligne de base ; le balisage créole produit un élément par morceau.
  if not hasMarkup(s):
    svg.plainText(x, y, s, anchor, size, bold, italic, fill, underline, mono)
    return
  let runs = parseRuns(s, size, bold, italic, underline, mono, fill)
  var total = 0.0
  for r in runs: total += runWidth(r)
  var cx = case anchor
    of "middle": x - total / 2
    of "end": x - total
    else: x
  for r in runs:
    if r.sprite.len > 0: svg.drawSprite(r, cx + 1, y)
    else:
      # SVG écrase les espaces de bord d'un élément : ils deviennent un décalage
      let shown = r.text.strip(leading = true, trailing = false)
      let lead = plainWidth(r.text[0 ..< r.text.len - shown.len], r.size, r.bold)
      svg.plainText(cx + lead, y, shown, "start", r.size, r.bold, r.italic, r.color, r.underline, r.mono)
    cx += runWidth(r)

proc textBlock*(svg: var Svg, x, top: float, lines: seq[string], anchor = "start",
                size = FontSize, bold = false, italic = false, fill = Ink) =
  ## Bloc multiligne dont `top` est le haut de la première ligne.
  let lh = size * 1.25
  for i, l in lines:
    svg.text(x, top + lh * float(i) + size * 0.95, l, anchor, size, bold, italic, fill)

proc blockHeight*(n: int, size = FontSize): float = float(n) * size * 1.25

proc note*(svg: var Svg, x, y, w, h: float, fill = NoteFill, stroke = NoteStroke) =
  let f = 9.0
  svg.path("M" & fmtF(x) & " " & fmtF(y) & " H" & fmtF(x + w - f) & " L" & fmtF(x + w) & " " &
    fmtF(y + f) & " V" & fmtF(y + h) & " H" & fmtF(x) & " Z", fill, stroke)
  svg.path("M" & fmtF(x + w - f) & " " & fmtF(y) & " V" & fmtF(y + f) & " H" & fmtF(x + w), "none", stroke)

proc arrowHead*(svg: var Svg, x, y, angle: float, style: string, color = Stroke) =
  ## Pointe en (x, y) orientée selon `angle` (direction de la flèche).
  let ca = cos(angle)
  let sa = sin(angle)
  proc pt(dx, dy: float): (float, float) =
    (x + dx * ca - dy * sa, y + dx * sa + dy * ca)
  case style
  of "open":
    let a = pt(-11, -5.5)
    let b = pt(-11, 5.5)
    svg.path("M" & fmtF(a[0]) & " " & fmtF(a[1]) & " L" & fmtF(x) & " " & fmtF(y) & " L" &
      fmtF(b[0]) & " " & fmtF(b[1]), "none", color)
  of "filled":
    svg.polygon([pt(0, 0), pt(-11, -5), pt(-8, 0), pt(-11, 5)], color, color)
  of "triangle":
    svg.polygon([pt(0, 0), pt(-14, -8), pt(-14, 8)], Background, color)
  of "diamond", "fdiamond":
    svg.polygon([pt(0, 0), pt(-8, -5.5), pt(-16, 0), pt(-8, 5.5)],
      (if style == "fdiamond": color else: Background), color)
  of "circle":
    let c = pt(-5, 0)
    svg.circle(c[0], c[1], 5, Background, color)
  of "cross":
    let a = pt(-10, -5)
    let b = pt(0, 5)
    let c = pt(-10, 5)
    let d = pt(0, -5)
    svg.line(a[0], a[1], b[0], b[1], color, 1.6)
    svg.line(c[0], c[1], d[0], d[1], color, 1.6)
  of "half":
    let a = pt(-11, -5.5)
    svg.path("M" & fmtF(a[0]) & " " & fmtF(a[1]) & " L" & fmtF(x) & " " & fmtF(y), "none", color)
  of "plus":
    let c = pt(-6, 0)
    svg.circle(c[0], c[1], 6, Background, color)
    let a = pt(-12, 0)
    let b = pt(-6, -6)
    let d = pt(-6, 6)
    svg.line(a[0], a[1], x, y, color)
    svg.line(b[0], b[1], d[0], d[1], color)
  of "crowfoot":
    let a = pt(-10, 0)
    let b = pt(0, -6)
    let d = pt(0, 6)
    svg.line(a[0], a[1], b[0], b[1], color)
    svg.line(a[0], a[1], d[0], d[1], color)
    svg.line(a[0], a[1], x, y, color)
  of "socket":
    let c = pt(-7, 0)
    svg.path("M" & fmtF(pt(-7, -7)[0]) & " " & fmtF(pt(-7, -7)[1]) & " A7 7 0 0 1 " &
      fmtF(pt(-7, 7)[0]) & " " & fmtF(pt(-7, 7)[1]), "none", color)
    discard c
  else: discard

proc finish*(svg: Svg, w, h: float): string =
  let W = ceil(w)
  let H = ceil(h)
  result = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n" &
    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"" & fmtF(W) & "\" height=\"" & fmtF(H) &
    "\" viewBox=\"0 0 " & fmtF(W) & " " & fmtF(H) & "\">\n"
  if svg.defs.len > 0: result.add "<defs>\n" & svg.defs & "</defs>\n"
  result.add "<rect x=\"0\" y=\"0\" width=\"" & fmtF(W) & "\" height=\"" & fmtF(H) & "\" fill=\"" & Background & "\"/>\n"
  result.add svg.body
  result.add "</svg>\n"

# ---------------------------------------------------------------- lexique

type Scanner* = object
  s*: string
  pos*: int

proc initScanner*(s: string): Scanner = Scanner(s: s, pos: 0)
proc eof*(sc: Scanner): bool = sc.pos >= sc.s.len
proc rest*(sc: Scanner): string = (if sc.pos < sc.s.len: sc.s[sc.pos .. ^1] else: "")
proc peek*(sc: Scanner, off = 0): char =
  (if sc.pos + off < sc.s.len: sc.s[sc.pos + off] else: '\0')

proc skipWs*(sc: var Scanner) =
  while sc.pos < sc.s.len and sc.s[sc.pos] in {' ', '\t'}: inc sc.pos

proc accept*(sc: var Scanner, lit: string): bool =
  if sc.s.continuesWith(lit, sc.pos):
    sc.pos += lit.len
    true
  else: false

proc acceptWord*(sc: var Scanner, word: string): bool =
  ## Mot-clé insensible à la casse suivi d'une frontière.
  let n = word.len
  if sc.pos + n > sc.s.len: return false
  if cmpIgnoreCase(sc.s[sc.pos ..< sc.pos + n], word) != 0: return false
  if sc.pos + n < sc.s.len and sc.s[sc.pos + n] in IdentChars: return false
  sc.pos += n
  true

proc readQuoted*(sc: var Scanner): string =
  ## Lit "..." ; renvoie le contenu sans guillemets.
  if sc.peek != '"': return ""
  inc sc.pos
  let start = sc.pos
  while sc.pos < sc.s.len and sc.s[sc.pos] != '"': inc sc.pos
  result = sc.s[start ..< min(sc.pos, sc.s.len)]
  if sc.pos < sc.s.len: inc sc.pos

const NameChars* = IdentChars + {'.', ':', '$', '-', '\x80'..'\xFF'}

proc readName*(sc: var Scanner, allowed = IdentChars + {'.', ':', '$', '\x80'..'\xFF'}): string =
  let start = sc.pos
  while sc.pos < sc.s.len and sc.s[sc.pos] in allowed: inc sc.pos
  sc.s[start ..< sc.pos]

proc readDelimited*(sc: var Scanner, open, close: char): string =
  ## Lit open...close avec imbrication ; le curseur doit être sur `open`.
  if sc.peek != open: return ""
  var depth = 0
  let start = sc.pos + 1
  while sc.pos < sc.s.len:
    let c = sc.s[sc.pos]
    if c == open and (open != close or depth == 0): inc depth
    elif c == close:
      dec depth
      if depth == 0:
        result = sc.s[start ..< sc.pos]
        inc sc.pos
        return
    inc sc.pos
  result = sc.s[start ..< sc.s.len]

proc startsWithWord*(s, word: string): bool =
  let t = s
  if t.len < word.len: return false
  if cmpIgnoreCase(t[0 ..< word.len], word) != 0: return false
  t.len == word.len or t[word.len] notin IdentChars

proc afterWord*(s, word: string): string = s[word.len .. ^1].strip()

proc lowerStart*(s: string, n: int): string = s[0 ..< min(n, s.len)].toLowerAscii()

proc splitStereotype*(s: string): (string, string) =
  ## "Nom <<stéréo>>" → ("Nom", "stéréo").
  let a = s.find("<<")
  if a < 0: return (s.strip(), "")
  let b = s.find(">>", a + 2)
  if b < 0: return (s.strip(), "")
  ((s[0 ..< a] & s[b + 2 .. ^1]).strip(), s[a + 2 ..< b].strip())

proc stripColor*(s: string): (string, string) =
  ## Enlève une couleur finale #xxx (hors guillemets).
  var t = s.strip()
  var quote = false
  var idx = -1
  for i, c in t:
    if c == '"': quote = not quote
    elif c == '#' and not quote and (i == 0 or t[i - 1] in {' ', '\t'}): idx = i
  if idx < 0: return (t, "")
  let tail = t[idx .. ^1]
  var token = tail
  let sp = tail.find({' ', '\t'})
  if sp >= 0: token = tail[0 ..< sp]
  (t[0 ..< idx].strip() & (if sp >= 0: " " & tail[sp .. ^1].strip() else: ""), colorValue(token.split(';')[0]))
