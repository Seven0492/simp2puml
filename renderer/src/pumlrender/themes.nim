## Thèmes `!theme` et réglages `skinparam` / `<style>` : ils modifient la palette
## du rendu en cours (voir `common.nim`). Les thèmes intégrés sont des palettes
## relevées sur ceux de PlantUML ; aucun fichier n'est lu.
import std/[strutils, tables]
import common

type Palette = tuple[bg, fill, header, line, text, note, noteLine, arrow: string]

# Couleurs relevées sur les thèmes de PlantUML 1.2026.9 (première teinte d'un dégradé).
const Themes: array[43, (string, Palette)] = [
  ("amiga", ("#0B58A8", "#0B58A8", "#0B58A8", "#FFFFFF", "#FFFFFF", "#0B58A8", "#FFFFFF", "#FFFFFF")),
  ("aws-orange", ("#FFFFFF", "#F18E3E", "#F18E3E", "#EC7211", "#FFFFFF", "#0073BB", "#005C96", "#EC7211")),
  ("black-knight", ("#FFFFFF", "#3A3A3A", "#3A3A3A", "#262626", "#FFF200", "#0060A8", "#004D86", "#303030")),
  ("bluegray", ("#FFFFFF", "#33B2E2", "#33B2E2", "#33B2E2", "#D2D2D2", "#3786BE", "#0568AE", "#009FDB")),
  ("blueprint", ("#003153", "#003153", "#003153", "#D9D3D0", "#D9D3D0", "#003153", "#D9D3D0", "#D9D3D0")),
  ("carbon-gray", ("#FFFFFF", "#F4F4F4", "#F4F4F4", "#C6C6C6", "#262626", "#FFFFFF", "#262626", "#C6C6C6")),
  ("cerulean", ("#FFFFFF", "#59B6EC", "#2FA4E7", "#2FA4E7", "#F8F9FA", "#35638F", "#033C73", "#2FA4E7")),
  ("cerulean-outline", ("#FFFFFF", "#FFFFFF", "#2FA4E7", "#2FA4E7", "#343A40", "#FBFB77", "#033C73", "#2FA4E7")),
  ("cloudscape-design", ("#FFFFFF", "#1388F5", "#1388F5", "#0972D3", "#D1D5DB", "#0972D3", "#000000", "#A4AAB0")),
  ("crt-amber", ("#282828", "#282828", "#282828", "#FFB000", "#FFB000", "#282828", "#FFB000", "#FFB000")),
  ("crt-green", ("#282828", "#282828", "#282828", "#33FF33", "#33FF33", "#282828", "#33FF33", "#33FF33")),
  ("cyborg", ("#FFFFFF", "#2A9FD6", "#227FAB", "#55B2DE", "#222222", "#9933CC", "#AD5CD6", "#2A9FD6")),
  ("cyborg-outline", ("#FFFFFF", "#FFFFFF", "#227FAB", "#55B2DE", "#ADAFAE", "#FBFB77", "#AD5CD6", "#2A9FD6")),
  ("hacker", ("#FFFFFF", "#D3F198", "#D3F198", "#D3F198", "#151515", "#BB91B2", "#885E7F", "#B5E853")),
  ("lightgray", ("#FFFFFF", "#F3F3F3", "#F3F3F3", "#F3F3F3", "#303030", "#5C85AD", "#336699", "#E2E2E2")),
  ("mars", ("#F9F9F9", "#F1F1F1", "#F1F1F1", "#181818", "#000000", "#E5E5E5", "#191919", "#191919")),
  ("materia", ("#FFFFFF", "#4DABF5", "#2196F3", "#2196F3", "#FFFFFF", "#B052C0", "#9C27B0", "#2196F3")),
  ("materia-outline", ("#FFFFFF", "#FFFFFF", "#2196F3", "#2196F3", "#222222", "#FBFB77", "#9C27B0", "#2196F3")),
  ("metal", ("#FFFFFF", "#E5E4E2", "#EAE9E8", "#B7B6B5", "#4D4D4D", "#C0D0E5", "#8D9DB2", "#E5E4E2")),
  ("mimeograph", ("#D9D3D0", "#D9D3D0", "#D9D3D0", "#9275B6", "#9275B6", "#D9D3D0", "#9275B6", "#9275B6")),
  ("minty", ("#FFFFFF", "#86C8B5", "#86C8B5", "#78C2AD", "#343A40", "#6CC3D5", "#61B0C0", "#78C2AD")),
  ("mono", ("#FFFFFF", "#FFFFFF", "#FFFFFF", "#000000", "#000000", "#FFFFFF", "#000000", "#000000")),
  ("plain", ("#FFFFFF", "#FFFFFF", "#FFFFFF", "#000000", "#000000", "#FFFFFF", "#000000", "#000000")),
  ("reddress-darkblue", ("#777777", "#2E2E2E", "#2E2E2E", "#1B1B1B", "#AAAAAA", "#1A66C2", "#002642", "#FFFFFF")),
  ("reddress-darkgreen", ("#777777", "#2E2E2E", "#2E2E2E", "#1B1B1B", "#AAAAAA", "#228811", "#113300", "#FFFFFF")),
  ("reddress-darkorange", ("#777777", "#2E2E2E", "#2E2E2E", "#1B1B1B", "#AAAAAA", "#BB6600", "#662200", "#FFFFFF")),
  ("reddress-darkred", ("#777777", "#2E2E2E", "#2E2E2E", "#1B1B1B", "#AAAAAA", "#880000", "#330000", "#FFFFFF")),
  ("reddress-lightblue", ("#FFFFFF", "#CCCCCC", "#CCCCCC", "#AAAAAA", "#333333", "#2A86E2", "#1A66C2", "#000000")),
  ("reddress-lightgreen", ("#FFFFFF", "#CCCCCC", "#CCCCCC", "#AAAAAA", "#333333", "#55BB33", "#338822", "#000000")),
  ("reddress-lightorange", ("#FFFFFF", "#CCCCCC", "#CCCCCC", "#AAAAAA", "#333333", "#FF8800", "#BB6600", "#000000")),
  ("reddress-lightred", ("#FFFFFF", "#CCCCCC", "#CCCCCC", "#AAAAAA", "#333333", "#CC0033", "#AA0033", "#000000")),
  ("sandstone", ("#FFFFFF", "#325D88", "#476D94", "#2D547A", "#FFFFFF", "#29ABE0", "#259ACA", "#325D88")),
  ("silver", ("#FFFFFF", "#EEEEEE", "#F1F1F1", "#F1F1F1", "#303030", "#5C85AD", "#336699", "#EEEEEE")),
  ("sketchy", ("#FFFFFF", "#FFFFFF", "#474747", "#000000", "#FFFFFF", "#17A2B8", "#1592A6", "#333333")),
  ("sketchy-outline", ("#FFFFFF", "#2E2E2E", "#2E2E2E", "#000000", "#555555", "#17A2B8", "#1592A6", "#333333")),
  ("spacelab", ("#FFFFFF", "#7C9AB9", "#7C9AB9", "#304D6D", "#EEEEEE", "#70B8F7", "#246BAA", "#446E9B")),
  ("spacelab-white", ("#FFFFFF", "#7C9AB9", "#7C9AB9", "#304D6D", "#EEEEEE", "#70B8F7", "#246BAA", "#446E9B")),
  ("sunlust", ("#FDF6E3", "#FFE0E0", "#FFE0E0", "#DC322F", "#000000", "#FFFFCC", "#B58900", "#657B83")),
  ("superhero", ("#FFFFFF", "#DF691A", "#B25415", "#E58748", "#4E5D6C", "#5BC0DE", "#7CCDE5", "#DF691A")),
  ("superhero-outline", ("#FFFFFF", "#FFFFFF", "#B25415", "#E58748", "#4E5D6C", "#FBFB77", "#7CCDE5", "#DF691A")),
  ("toy", ("#DDDDDD", "#F1F1F1", "#F1F1F1", "#181818", "#000000", "#90EE90", "#454645", "#454645")),
  ("united", ("#FFFFFF", "#E95420", "#EB6536", "#D24C1D", "#772953", "#17A2B8", "#1592A6", "#E95420")),
  ("vibrant", ("#FFFFFF", "#F1F1F1", "#F1F1F1", "#181818", "#000000", "#7FFFD4", "#454645", "#454645")),
]

proc applyTheme*(name: string): bool =
  for (key, p) in Themes:
    if key == name.toLowerAscii():
      Background = p.bg
      FrameFill = p.bg
      Fill = p.fill
      HeaderFill = p.header
      Stroke = p.line
      Ink = p.text
      NoteFill = p.note
      NoteStroke = p.noteLine
      Muted = p.text
      Accent = p.line
      ArrowColor = p.arrow
      return true

const KnownElements = ["sequencelifeline", "sequencearrow", "sequencegroup", "sequencebox", "sequencedivider",
  "sequencereference", "sequenceparticipant", "activitydiamond", "activitybar", "activity", "participant",
  "usecase", "interface", "component", "rectangle", "partition", "collections", "boundary", "control",
  "database", "artifact", "storage", "package", "archimate", "object", "folder", "entity", "cloud", "queue",
  "stack", "actor", "agent", "state", "class", "frame", "arrow", "card", "file", "node", "note", "map"]

proc setStyleProp*(element, prop, value: string): bool =
  ## `element` vide : tout le diagramme. Faux si le réglage n'est pas pris en charge.
  let p = prop.toLowerAscii().replace("colour", "color")
  let color = colorValue(value)
  let e = element.toLowerAscii()
  template slot(field: untyped) =
    if e.len == 0:
      return false
    if not Elements.hasKey(e): Elements[e] = ElementStyle()
    Elements[e].field = color
    return color.len > 0
  case p
  of "backgroundcolor":
    if color.len == 0: return false
    case e
    of "": Background = color; FrameFill = color; return true
    of "note": NoteFill = color; return true
    else: slot(bg)
  of "bordercolor", "linecolor", "color":
    if color.len == 0: return false
    case e
    of "": Stroke = color; return true
    of "note": NoteStroke = color; return true
    of "arrow", "sequencearrow": ArrowColor = color; return true
    else: slot(border)
  of "fontcolor":
    if color.len == 0: return false
    if e.len == 0: Ink = color; return true
    slot(font)
  of "borderthickness", "linethickness", "thickness":
    var v = 0.0
    try: v = clamp(parseFloat(value.strip()), 0.1, 12.0) except ValueError: return false
    if e.len == 0: LineWidth = v; return true
    if not Elements.hasKey(e): Elements[e] = ElementStyle()
    Elements[e].thickness = v
    return true
  of "fontsize":
    # la taille globale entre dans le calcul des boîtes ; pas celle d'un seul élément
    if e.len > 0: return false
    try: FontSize = clamp(parseFloat(value.strip()), 6.0, 48.0) except ValueError: return false
    LineHeight = FontSize * 1.23
    return true
  of "fontname":
    if e.len > 0: return false
    let name = value.strip().strip(chars = {'"'})
    for c in name:
      if c notin {'a'..'z', 'A'..'Z', '0'..'9', ' ', '-', '_'}: return false
    FontFamily = name & ", DejaVu Sans, sans-serif"
    return true
  else: return false

proc applySkinparam*(key, value: string): bool =
  ## « classBackgroundColor », « defaultFontSize », « backgroundColor »…
  let k = key.toLowerAscii()
  case k
  of "backgroundcolor": return setStyleProp("", "backgroundcolor", value)
  of "arrowcolor": return setStyleProp("arrow", "color", value)
  of "monochrome":
    if value.strip().toLowerAscii() == "true": discard applyTheme("mono")
    elif value.strip().toLowerAscii() == "reverse":
      discard applyTheme("mono")
      (Background, FrameFill, Fill, HeaderFill, NoteFill) = ("#000000", "#000000", "#000000", "#333333", "#222222")
      (Stroke, NoteStroke, Ink, Muted) = ("#FFFFFF", "#FFFFFF", "#FFFFFF", "#BBBBBB")
    return true
  else: discard
  if k.startsWith("default"): return setStyleProp("", k[7 .. ^1], value)
  for element in KnownElements:
    if k.startsWith(element) and k.len > element.len:
      return setStyleProp(element, k[element.len .. ^1], value)
  false

proc applyStyleBlock*(text: string, line: int, ctx: Context) =
  ## Bloc `<style>` : sélecteurs imbriqués, le plus précis désigne l'élément.
  var path: seq[string]
  var tok = ""
  proc element(): string =
    for i in countdown(path.high, 0):
      let p = path[i].toLowerAscii()
      if p in KnownElements or p == "lifeline" or p == "group": return (if p == "lifeline": "sequencelifeline" elif p == "group": "sequencegroup" else: p)
    ""
  proc declaration(raw: string) =
    let t = raw.strip()
    if t.len == 0: return
    let parts = t.splitWhitespace(1)
    if parts.len == 2 and not setStyleProp(element(), parts[0], parts[1]):
      ctx.info(line, "style non pris en charge : " & t)
  for c in text:
    case c
    of '{':
      let sel = tok.strip()
      if sel.startsWith("."):
        ctx.info(line, "style de stéréotype non pris en charge : " & sel)
      path.add sel
      tok = ""
    of '}':
      declaration(tok)
      tok = ""
      if path.len > 0: discard path.pop()
    of '\n', ';':
      declaration(tok)
      tok = ""
    else: tok.add c

proc parseSpriteRows*(name, size: string, rows: seq[string], line: int, ctx: Context) =
  ## « $nom [16x16/16] » : une ligne de chiffres hexadécimaux par rangée, 0 transparent.
  var levels = 16
  let spec = size.strip(chars = {'[', ']', ' '})
  if spec.endsWith("z"):
    ctx.warn(line, "sprite compressé non pris en charge : " & name)
    return
  let slash = spec.find('/')
  if slash > 0:
    try: levels = clamp(parseInt(spec[slash + 1 .. ^1]), 2, 16)
    except ValueError: discard
  var width = 0
  for r in rows: width = max(width, r.len)
  if width == 0 or rows.len == 0 or width * rows.len > 4_000_000:
    ctx.warn(line, "sprite vide ou trop grand : " & name)
    return
  var sp = Sprite(width: width, height: rows.len, levels: levels, pixels: newSeq[int](width * rows.len))
  for y, r in rows:
    for x, c in r:
      if c notin HexDigits:
        ctx.warn(line, "sprite illisible : " & name)
        return
      sp.pixels[y * width + x] = parseHexInt($c)
  Sprites[name.strip(chars = {'$', ' '})] = sp
