## Types partagés : diagnostics, régions cliquables, métriques de texte,
## construction SVG et petit analyseur lexical sans expressions régulières.
import std/[strutils, math]
from std/unicode import Rune, runes, runeAt, runeLenAt, validateUtf8

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

  Context* = ref object
    diags*: seq[Diagnostic]
    regions*: seq[Region]

  Meta* = object
    title*, header*, footer*, caption*, legend*: string
    titleLine*, legendLine*: int
    leftToRight*: bool
    legendAlign*: string

  Svg* = object
    body*: string
    defs*: string

const
  FontFamily* = "DejaVu Sans, Liberation Sans, Arial, Helvetica, sans-serif"
  MonoFamily* = "DejaVu Sans Mono, Liberation Mono, monospace"
  FontSize* = 13.0
  LineHeight* = 16.0
  Ink* = "#1F2430"
  Stroke* = "#3B4252"
  Fill* = "#F7F8FC"
  HeaderFill* = "#E8ECF6"
  NoteFill* = "#FFF6BF"
  NoteStroke* = "#B49B2F"
  Accent* = "#3A6EA5"
  Muted* = "#6B7280"
  FrameFill* = "#FBFBFD"

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

proc hasErrors*(ctx: Context): bool =
  for d in ctx.diags:
    if d.severity == sevError: return true

# ---------------------------------------------------------------- texte

proc runeWidth(r: Rune): float =
  let c = int(r)
  if c < 128:
    let ch = char(c)
    case ch
    of 'i', 'l', 'j', '\'', '|', '.', ',', ':', ';', '!', 'I': 0.30
    of 'f', 't', 'r', '(', ')', '[', ']', '{', '}', ' ', '"', '-': 0.38
    of 'm', 'w', 'M', 'W': 0.88
    of 'A'..'H', 'J'..'L', 'N'..'V', 'X'..'Z', '@', '%', '&': 0.68
    of '0'..'9': 0.58
    else: 0.58
  elif c >= 0x1100 and (c <= 0x115F or (c >= 0x2E80 and c <= 0xA4CF) or
      (c >= 0xAC00 and c <= 0xD7A3) or (c >= 0xF900 and c <= 0xFAFF) or
      (c >= 0xFE30 and c <= 0xFE4F) or (c >= 0xFF00 and c <= 0xFF60) or
      (c >= 0x1F300 and c <= 0x1FAFF)):
    1.0
  else:
    0.6

proc textWidth*(s: string, size = FontSize, bold = false): float =
  var w = 0.0
  for r in s.runes: w += runeWidth(r)
  result = w * size * (if bold: 1.1 else: 1.03)

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
  ## Convertit les séquences PlantUML \n et \t et enlève le balisage créole simple.
  result = s.replace("\\n", "\n").replace("\\t", "    ").replace("%n()", "\n")
  for m in ["**", "//", "__", "\"\"", "~~"]:
    result = result.replace(m, "")

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
  if t.len == 0: return ""
  for c in t:
    if c notin {'0'..'9', 'a'..'z', 'A'..'Z'}: return ""
  var hex = t.len in [3, 6, 8]
  for c in t:
    if c notin HexDigits: hex = false
  if hex: "#" & t else: t.toLowerAscii()

# ---------------------------------------------------------------- SVG

proc add*(svg: var Svg, s: string) = svg.body.add s

proc rect*(svg: var Svg, x, y, w, h: float, fill = Fill, stroke = Stroke,
           rx = 0.0, sw = 1.2, dash = "", extra = "") =
  svg.add "<rect x=\"" & fmtF(x) & "\" y=\"" & fmtF(y) & "\" width=\"" & fmtF(w) &
    "\" height=\"" & fmtF(h) & "\""
  if rx > 0: svg.add " rx=\"" & fmtF(rx) & "\""
  svg.add " fill=\"" & fill & "\" stroke=\"" & stroke & "\" stroke-width=\"" & fmtF(sw) & "\""
  if dash.len > 0: svg.add " stroke-dasharray=\"" & dash & "\""
  if extra.len > 0: svg.add " " & extra
  svg.add "/>\n"

proc line*(svg: var Svg, x1, y1, x2, y2: float, stroke = Stroke, sw = 1.2, dash = "") =
  svg.add "<line x1=\"" & fmtF(x1) & "\" y1=\"" & fmtF(y1) & "\" x2=\"" & fmtF(x2) &
    "\" y2=\"" & fmtF(y2) & "\" stroke=\"" & stroke & "\" stroke-width=\"" & fmtF(sw) & "\""
  if dash.len > 0: svg.add " stroke-dasharray=\"" & dash & "\""
  svg.add "/>\n"

proc circle*(svg: var Svg, cx, cy, r: float, fill = Fill, stroke = Stroke, sw = 1.2) =
  svg.add "<circle cx=\"" & fmtF(cx) & "\" cy=\"" & fmtF(cy) & "\" r=\"" & fmtF(r) &
    "\" fill=\"" & fill & "\" stroke=\"" & stroke & "\" stroke-width=\"" & fmtF(sw) & "\"/>\n"

proc ellipse*(svg: var Svg, cx, cy, rx, ry: float, fill = Fill, stroke = Stroke, sw = 1.2) =
  svg.add "<ellipse cx=\"" & fmtF(cx) & "\" cy=\"" & fmtF(cy) & "\" rx=\"" & fmtF(rx) &
    "\" ry=\"" & fmtF(ry) & "\" fill=\"" & fill & "\" stroke=\"" & stroke &
    "\" stroke-width=\"" & fmtF(sw) & "\"/>\n"

proc path*(svg: var Svg, d: string, fill = "none", stroke = Stroke, sw = 1.2, dash = "") =
  svg.add "<path d=\"" & d & "\" fill=\"" & fill & "\" stroke=\"" & stroke &
    "\" stroke-width=\"" & fmtF(sw) & "\" stroke-linejoin=\"round\""
  if dash.len > 0: svg.add " stroke-dasharray=\"" & dash & "\""
  svg.add "/>\n"

proc polygon*(svg: var Svg, pts: openArray[(float, float)], fill = Fill, stroke = Stroke, sw = 1.2) =
  var s = ""
  for i, p in pts:
    if i > 0: s.add ' '
    s.add fmtF(p[0]) & "," & fmtF(p[1])
  svg.add "<polygon points=\"" & s & "\" fill=\"" & fill & "\" stroke=\"" & stroke &
    "\" stroke-width=\"" & fmtF(sw) & "\" stroke-linejoin=\"round\"/>\n"

proc text*(svg: var Svg, x, y: float, s: string, anchor = "start", size = FontSize,
           bold = false, italic = false, fill = Ink, underline = false, mono = false) =
  ## y est la ligne de base.
  svg.add "<text x=\"" & fmtF(x) & "\" y=\"" & fmtF(y) & "\" font-family=\"" &
    (if mono: MonoFamily else: FontFamily) & "\" font-size=\"" & fmtF(size) & "\" fill=\"" & fill & "\""
  if anchor != "start": svg.add " text-anchor=\"" & anchor & "\""
  if bold: svg.add " font-weight=\"bold\""
  if italic: svg.add " font-style=\"italic\""
  if underline: svg.add " text-decoration=\"underline\""
  svg.add ">" & xmlEscape(s) & "</text>\n"

proc textBlock*(svg: var Svg, x, top: float, lines: seq[string], anchor = "start",
                size = FontSize, bold = false, italic = false, fill = Ink) =
  ## Bloc multiligne dont `top` est le haut de la première ligne.
  let lh = size * 1.25
  for i, l in lines:
    svg.text(x, top + lh * float(i) + size * 0.95, l, anchor, size, bold, italic, fill)

proc blockHeight*(n: int, size = FontSize): float = float(n) * size * 1.25

proc note*(svg: var Svg, x, y, w, h: float, fill = NoteFill) =
  let f = 9.0
  svg.path("M" & fmtF(x) & " " & fmtF(y) & " H" & fmtF(x + w - f) & " L" & fmtF(x + w) & " " &
    fmtF(y + f) & " V" & fmtF(y + h) & " H" & fmtF(x) & " Z", fill, NoteStroke)
  svg.path("M" & fmtF(x + w - f) & " " & fmtF(y) & " V" & fmtF(y + f) & " H" & fmtF(x + w), "none", NoteStroke)

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
    svg.polygon([pt(0, 0), pt(-14, -8), pt(-14, 8)], "#FFFFFF", color)
  of "diamond", "fdiamond":
    svg.polygon([pt(0, 0), pt(-8, -5.5), pt(-16, 0), pt(-8, 5.5)],
      (if style == "fdiamond": color else: "#FFFFFF"), color)
  of "circle":
    let c = pt(-5, 0)
    svg.circle(c[0], c[1], 5, "#FFFFFF", color)
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
    svg.circle(c[0], c[1], 6, "#FFFFFF", color)
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
  result.add "<rect x=\"0\" y=\"0\" width=\"" & fmtF(W) & "\" height=\"" & fmtF(H) & "\" fill=\"#FFFFFF\"/>\n"
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
