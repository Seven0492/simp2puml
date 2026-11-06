## Diagrammes « graphe » : classes, objets, cas d'utilisation, composants,
## déploiement, états et ancienne syntaxe d'activité. Les conteneurs
## (packages, nœuds, états composites…) sont mis en page récursivement.
import std/[strutils, tables, math, algorithm]
import common, preprocess, graphlayout

type
  Shape* = enum
    shClass, shObject, shMap, shUsecase, shActor, shComponent, shInterface, shNode,
    shDatabase, shCloud, shArtifact, shFile, shFolder, shFrame, shPackage, shRectangle,
    shQueue, shStack, shCard, shAgent, shStorage, shPerson, shLabel, shHexagon,
    shBoundary, shControl, shEntity, shCollections, shState, shStart, shEnd, shChoice,
    shForkBar, shHistory, shDeepHistory, shNote, shActivity, shCircle, shDiamond, shPort,
    shTogether, shPoint

  Entity* = ref object
    id*, label*: string
    shape*: Shape
    keyword*: string
    stereo*: string
    color*: string
    sections*: seq[seq[string]]
    ## Lignes du source de chaque membre, dans l'ordre (un même texte peut revenir).
    memberLines*: Table[string, seq[int]]
    explicitSections*: bool
    line*: int
    parent*: int
    isContainer*: bool
    w*, h*, x*, y*: float   ## x, y absolus après mise en page
    relX, relY: float
    contentW, contentH: float

  Rel* = object
    a*, b*: int
    leftHead*, rightHead*: string
    dashed*, hidden*, bold*: bool
    minlen*: int
    up*: bool
    label*, multA*, multB*: string
    color*: string
    line*: int
    points*: seq[(float, float)]

  GraphDiagram* = object
    kind*: DiagramKind
    entities*: seq[Entity]
    rels*: seq[Rel]
    byId*: Table[string, int]
    leftToRight*: bool
    hideCircle*, hideEmpty*, hideMembers*, hideStereo*, hideMethods*, hideFields*: bool
    lastEntity: int

const
  ClassKeywords = ["abstract class", "abstract", "class", "interface", "enum", "annotation",
    "protocol", "struct", "exception", "metaclass", "stereotype", "entity", "dataclass", "record"]
  ContainerKeywords = ["archimate", "package", "namespace", "node", "frame", "folder", "rectangle", "cloud",
    "database", "component", "card", "storage", "artifact", "hexagon", "together", "partition",
    "queue", "stack", "file", "agent", "boundary", "person", "process"]
  OtherKeywords = ["usecase/", "usecase", "actor/", "actor", "object", "map", "state", "port", "portin",
    "portout", "control", "collections", "label", "circle", "diamond", "json"]

const SectionMark = "\x1F"  ## marque interne d'un séparateur titré (jamais affichée)

proc shapeFor(keyword: string): Shape =
  case keyword
  of "class", "abstract", "abstract class", "interface", "enum", "annotation", "protocol",
     "struct", "exception", "metaclass", "stereotype", "dataclass", "record": shClass
  of "entity": shClass
  of "object": shObject
  of "map", "json": shMap
  of "usecase", "usecase/": shUsecase
  of "actor", "actor/": shActor
  of "component": shComponent
  of "node": shNode
  of "database": shDatabase
  of "cloud": shCloud
  of "artifact": shArtifact
  of "file": shFile
  of "folder": shFolder
  of "frame": shFrame
  of "package", "namespace": shPackage
  of "rectangle", "process", "archimate": shRectangle
  of "queue": shQueue
  of "stack": shStack
  of "card": shCard
  of "agent": shAgent
  of "storage": shStorage
  of "person": shPerson
  of "label": shLabel
  of "hexagon": shHexagon
  of "boundary": shBoundary
  of "control": shControl
  of "collections": shCollections
  of "state": shState
  of "port", "portin", "portout": shPort
  of "circle": shCircle
  of "diamond": shDiamond
  of "together": shTogether
  of "partition": shPackage
  else: shRectangle

proc defaultShape(kind: DiagramKind): Shape =
  case kind
  of dkState: shState
  of dkUsecase: shActor
  of dkComponent: shInterface
  of dkObject: shObject
  of dkActivity: shActivity
  else: shClass

proc noteLine(e: Entity, member: string, line: int) =
  e.memberLines.mgetOrPut(member, @[]).add line

proc scopeOf(g: GraphDiagram, stack: seq[int]): int =
  if stack.len > 0: stack[^1] else: -1

proc isAncestorOrSelf(g: GraphDiagram, anc, idx: int): bool =
  ## `anc` est `idx` ou l'un de ses ancêtres (parcours borné : jamais de boucle).
  var cur = idx
  var steps = 0
  while cur >= 0 and steps <= g.entities.len:
    if cur == anc: return true
    cur = g.entities[cur].parent
    inc steps
  false

proc addEntity(g: var GraphDiagram, id, label: string, shape: Shape, line: int, parent: int,
               keyword = ""): int =
  let e = Entity(id: id, label: label, shape: shape, line: line, parent: parent, keyword: keyword)
  g.entities.add e
  result = g.entities.high
  g.byId[id] = result
  g.lastEntity = result

proc ensure(g: var GraphDiagram, id, label: string, shape: Shape, line: int, parent: int): int =
  if id in g.byId: return g.byId[id]
  g.addEntity(id, label, shape, line, parent)

type RefKind = enum rkName, rkStart, rkHistory, rkDeepHistory

proc parseRef(sc: var Scanner, kind: DiagramKind): (string, string, Shape, RefKind, bool) =
  ## (id, libellé, forme, nature, trouvé)
  sc.skipWs()
  let c = sc.peek
  if sc.accept("[*]") or sc.accept("(*)"):
    return ("", "", shStart, rkStart, true)
  if sc.accept("(*top)"):
    return ("", "", shStart, rkStart, true)
  if sc.accept("[H*]"): return ("", "", shDeepHistory, rkDeepHistory, true)
  if sc.accept("[H]"): return ("", "", shHistory, rkHistory, true)
  if c == '[':
    let t = sc.readDelimited('[', ']').strip()
    return (t, t, shComponent, rkName, t.len > 0)
  if c == '(' and sc.peek(1) == ')':
    sc.pos += 2
    sc.skipWs()
    var n = if sc.peek == '"': sc.readQuoted() else: sc.readName()
    return (n, n, shInterface, rkName, n.len > 0)
  if c == '(':
    let t = sc.readDelimited('(', ')').strip()
    return (t, t, shUsecase, rkName, t.len > 0)
  if c == ':' and kind != dkClass:
    let save = sc.pos
    inc sc.pos
    let start = sc.pos
    while sc.pos < sc.s.len and sc.s[sc.pos] notin {':', '\n'}: inc sc.pos
    if sc.peek == ':' and sc.pos > start:
      let t = sc.s[start ..< sc.pos].strip()
      inc sc.pos
      return (t, t, shActor, rkName, t.len > 0)
    sc.pos = save
    return ("", "", shClass, rkName, false)
  if c == '"':
    let t = sc.readQuoted()
    let sh = if kind == dkActivity: shActivity else: defaultShape(kind)
    return (t, t, sh, rkName, t.len > 0)
  let n = sc.readName(IdentChars + {'.', '$', '\x80'..'\xFF'} + (if kind == dkClass: {':'} else: {}))
  if n.len == 0: return ("", "", shClass, rkName, false)
  # Les noms se terminant par un tiret appartiennent à la flèche.
  var name = n
  while name.len > 1 and name[^1] in {'.', ':'}:
    name.setLen(name.len - 1)
    dec sc.pos
  (name, name, defaultShape(kind), rkName, true)

const DirWords = ["up", "down", "left", "right", "u", "d", "l", "r", "le", "ri", "do", "dow", "lef", "rig", "righ"]

type Arrow = object
  leftHead, rightHead: string
  dashed, hidden, bold: bool
  length: int
  dir: string
  color: string

proc headStyle(h: string): string =
  case h
  of "<|", "|>", "^": "triangle"
  of "<", ">", "<<", ">>": "open"
  of "*": "fdiamond"
  of "o": "diamond"
  of "#": "diamond"
  of "x": "cross"
  of "+": "plus"
  of "0", "(0", "0)": "circle"
  of "(", ")": "socket"
  of "{", "}", "}|", "|{", "}o", "o{": "crowfoot"
  of "||", "|o", "o|": "circle"
  else: ""

proc parseArrow(sc: var Scanner): (Arrow, bool) =
  var a: Arrow
  let save = sc.pos
  sc.skipWs()
  for h in ["<|", "}|", "}o", "|o", "||", "|{", "<<", "<", "*", "o", "#", "x", "}", "+", "^", "0)", "0", "("]:
    if sc.s.continuesWith(h, sc.pos):
      let after = sc.pos + h.len
      if after < sc.s.len and sc.s[after] in {'-', '.', '='}:
        a.leftHead = h
        sc.pos = after
        break
  var bodyLen = 0
  while not sc.eof:
    let c = sc.peek
    if c in {'-', '.', '='}:
      if c == '.': a.dashed = true
      if c == '=': a.bold = true
      inc bodyLen
      inc sc.pos
    elif c == '[':
      let opt = sc.readDelimited('[', ']').toLowerAscii()
      for part in opt.split({',', ';'}):
        let p = part.strip()
        if p == "hidden": a.hidden = true
        elif p in ["dashed", "dotted"]: a.dashed = true
        elif p == "bold" or p.startsWith("thickness"): a.bold = true
        elif p.startsWith("#"): a.color = colorValue(p.split({' '})[0])
    elif c in Letters and bodyLen > 0:
      let start = sc.pos
      var w = ""
      while not sc.eof and sc.peek in Letters:
        w.add sc.peek.toLowerAscii()
        inc sc.pos
      if w in DirWords and not sc.eof and sc.peek in {'-', '.', '=', '['}:
        a.dir = w[0 .. 0]
      else:
        sc.pos = start
        break
    else: break
  if bodyLen == 0:
    sc.pos = save
    return (a, false)
  for h in ["|>", "|{", "|o", "||", "o|", "o{", ">>", ">", "*", "o", "#", "x", "{", "+", "^", "(0", "0", ")"]:
    if sc.s.continuesWith(h, sc.pos):
      let after = sc.pos + h.len
      if after >= sc.s.len or sc.s[after] in {' ', '\t', '"', '[', '(', ':'} or h notin ["o", "x", "0"]:
        a.rightHead = h
        sc.pos = after
        break
  a.length = bodyLen
  (a, true)

proc cleanLabel(s: string): string =
  var t = s.strip()
  if t.startsWith("<") and not t.startsWith("<<"): t = "◀ " & t[1 .. ^1].strip()
  if t.endsWith(">") and not t.endsWith(">>"): t = t[0 .. ^2].strip() & " ▶"
  unescapeLabel(t)

proc refEntity(g: var GraphDiagram, r: (string, string, Shape, RefKind, bool), isSource: bool,
               line, scope: int): int =
  let (id, label, shape, rk, _) = r
  case rk
  of rkStart:
    let key = (if isSource: "__start_" else: "__end_") & $scope
    if key in g.byId: return g.byId[key]
    return g.addEntity(key, "", (if isSource: shStart else: shEnd), line, scope)
  of rkHistory, rkDeepHistory:
    let key = "__hist" & (if rk == rkDeepHistory: "d_" else: "_") & $scope
    if key in g.byId: return g.byId[key]
    return g.addEntity(key, "", shape, line, scope)
  of rkName:
    if id in g.byId: return g.byId[id]
    var lbl = label
    if shape == shActivity: lbl = unescapeLabel(label)
    g.addEntity(id, lbl, shape, line, scope)

proc tryRelation(g: var GraphDiagram, text: string, line, scope: int, ctx: Context): bool =
  var sc = initScanner(text)
  let left = parseRef(sc, g.kind)
  if not left[4]: return false
  sc.skipWs()
  var multA = ""
  if sc.peek == '"':
    multA = sc.readQuoted()
  let (arrow, ok) = parseArrow(sc)
  if not ok: return false
  sc.skipWs()
  var multB = ""
  if sc.peek == '"':
    let save = sc.pos
    multB = sc.readQuoted()
    sc.skipWs()
    if sc.eof or sc.peek == ':':
      # c'était la cible entre guillemets, pas une multiplicité
      sc.pos = save
      multB = ""
  let right = parseRef(sc, g.kind)
  if not right[4]: return false
  sc.skipWs()
  var label = ""
  if sc.accept(":"):
    label = sc.rest.strip()
  elif not sc.eof:
    let r = sc.rest.strip()
    if r.startsWith("as ") and g.kind == dkActivity:
      discard
    elif not r.startsWith("#") and not r.startsWith("[["):
      return false
  let ia = g.refEntity(left, true, line, scope)
  let ib = g.refEntity(right, false, line, scope)
  var rel = Rel(a: ia, b: ib, leftHead: headStyle(arrow.leftHead), rightHead: headStyle(arrow.rightHead),
    dashed: arrow.dashed, hidden: arrow.hidden, bold: arrow.bold, label: cleanLabel(label),
    multA: multA, multB: multB, color: arrow.color, line: line)
  case arrow.dir
  of "u": (rel.minlen = 1; rel.up = true)
  of "l", "r": rel.minlen = 0
  of "d": rel.minlen = max(1, arrow.length - 1)
  else: rel.minlen = if arrow.length <= 1: 0 else: arrow.length - 1
  if arrow.dir == "l": rel.up = true
  if arrow.rightHead in ["|>", "^"] and arrow.leftHead == "" and arrow.dir == "":
    rel.up = true  # « Enfant --|> Parent » : le parent reste au-dessus
  g.rels.add rel
  true

proc parseDecl(g: var GraphDiagram, text: string, line: int, scope: var seq[int],
               bodyOf: var int, ctx: Context): bool =
  # « archimate #Technology "x" as y » : la couleur vient avant le nom
  var text = text
  if text.toLowerAscii().startsWith("archimate #"):
    let parts = text.splitWhitespace(2)
    if parts.len == 3: text = "archimate " & parts[2] & " " & parts[1]
  let lw = text.toLowerAscii()
  var keyword = ""
  for kw in ClassKeywords:
    if lw.startsWithWord(kw) or (kw.endsWith("/") and lw.startsWith(kw)): (keyword = kw; break)
  if keyword == "":
    for kw in OtherKeywords:
      if (kw.endsWith("/") and lw.startsWith(kw)) or lw.startsWithWord(kw): (keyword = kw; break)
  if keyword == "":
    for kw in ContainerKeywords:
      if lw.startsWithWord(kw): (keyword = kw; break)
  if keyword == "": return false
  if keyword == "entity" and g.kind notin {dkClass, dkObject}: discard
  var sc = initScanner(text)
  sc.pos = keyword.len
  sc.skipWs()
  if keyword == "abstract":
    if sc.acceptWord("class"): (keyword = "abstract class"; sc.skipWs())
  if sc.eof and keyword notin ["together", "package", "namespace", "rectangle", "node", "frame"]:
    return false
  var label = ""
  var id = ""
  var shape = shapeFor(keyword)
  # nom
  let c = sc.peek
  if c == '"':
    label = sc.readQuoted()
  elif c == '[' and keyword in ["component", "node", "artifact", "rectangle"]:
    label = sc.readDelimited('[', ']')
  elif c == '(' and keyword.startsWith("usecase"):
    label = sc.readDelimited('(', ')')
  elif c == ':' and keyword.startsWith("actor"):
    inc sc.pos
    let st = sc.pos
    while not sc.eof and sc.peek != ':': inc sc.pos
    label = sc.s[st ..< sc.pos]
    if not sc.eof: inc sc.pos
  elif c == '{':
    label = ""
  else:
    let st = sc.pos
    var depth = 0
    while not sc.eof:
      let ch = sc.peek
      if ch == '<':
        if sc.peek(1) == '<' and depth == 0: break
        inc depth
      elif ch == '>' and depth > 0: dec depth
      elif depth == 0 and ch in {' ', '\t', '{', '#'}: break
      inc sc.pos
    label = sc.s[st ..< sc.pos]
  id = label
  # « class Foo<T> » : l'identité est Foo (les relations écrivent « Foo --> Bar »)
  if keyword in ClassKeywords and label.len > 0 and label[0] != '<':
    let lt = label.find('<')
    if lt > 0: id = label[0 ..< lt].strip()
  sc.skipWs()
  var stereo = ""
  var color = ""
  var opens = false
  var desc = ""
  var extends: seq[(string, bool)]
  while not sc.eof:
    sc.skipWs()
    if sc.eof: break
    if sc.acceptWord("as"):
      sc.skipWs()
      var alias = ""
      if sc.peek == '"':
        # forme  X as "Libellé"
        alias = id
        label = sc.readQuoted()
      elif sc.peek == '(':
        alias = sc.readDelimited('(', ')')
      elif sc.peek == '[':
        alias = sc.readDelimited('[', ']')
      elif sc.peek == ':':
        inc sc.pos
        let st = sc.pos
        while not sc.eof and sc.peek != ':': inc sc.pos
        alias = sc.s[st ..< sc.pos]
        if not sc.eof: inc sc.pos
      else:
        alias = sc.readName()
      id = alias
    elif sc.accept("<<"):
      let st = sc.pos
      let e = sc.s.find(">>", st)
      if e < 0: break
      stereo = sc.s[st ..< e].strip()
      sc.pos = e + 2
    elif sc.peek == '#':
      inc sc.pos
      let st = sc.pos
      while not sc.eof and sc.peek notin {' ', '\t', '{'}: inc sc.pos
      color = colorValue(sc.s[st ..< sc.pos].split({';', '-', '/'})[0])
    elif sc.peek == '{':
      opens = true
      inc sc.pos
      let r = sc.rest.strip()
      if r.endsWith("}") and keyword in ClassKeywords:
        # corps sur une ligne
        desc = r[0 .. ^2]
        opens = false
      break
    elif sc.accept("[["):
      let e = sc.s.find("]]", sc.pos)
      sc.pos = if e < 0: sc.s.len else: e + 2
    elif sc.peek == ':':
      inc sc.pos
      desc = sc.rest.strip()
      break
    elif sc.acceptWord("extends") or sc.acceptWord("implements"):
      let impl = sc.s[0 ..< sc.pos].toLowerAscii().endsWith("implements")
      sc.skipWs()
      # « extends B implements C, D » : deux listes
      let r = sc.rest
      var segs = @[(r, impl)]
      let k = r.toLowerAscii().find("implements")
      if not impl and k > 0 and r[k - 1] in Whitespace and (k + 10 >= r.len or r[k + 10] in Whitespace + {'{'}):
        segs = @[(r[0 ..< k], false), (r[k + 10 .. ^1], true)]
      for (seg, isImpl) in segs:
        for part in seg.split({','}):
          var p = part.strip()
          if p.endsWith("{"):
            p = p[0 .. ^2].strip()
            opens = true
          if p.len > 0: extends.add (p, isImpl)
      break
    else:
      # mot inconnu (ex. options de style) : ignoré
      let st = sc.pos
      while not sc.eof and sc.peek notin {' ', '\t'}: inc sc.pos
      if sc.pos == st: inc sc.pos
  if id.len == 0:
    id = "__anon" & $line
  if label.len == 0 and keyword != "together": label = id
  case stereo.toLowerAscii()
  of "choice": shape = shChoice
  of "fork", "join": shape = shForkBar
  of "start": shape = shStart
  of "end": shape = shEnd
  of "history": shape = shHistory
  of "history*": shape = shDeepHistory
  of "entrypoint", "exitpoint", "inputpin", "outputpin", "expansioninput", "expansionoutput": shape = shPort
  else: discard
  if shape in {shChoice, shForkBar, shStart, shEnd, shHistory, shDeepHistory, shPort}: stereo = ""
  let isClassBody = keyword in ClassKeywords or keyword in ["object", "map", "json"]
  let parent = g.scopeOf(scope)
  var idx: int
  if id in g.byId:
    idx = g.byId[id]
    let e = g.entities[idx]
    e.label = unescapeLabel(label)
    e.shape = shape
    e.keyword = keyword
    e.line = line
    # jamais son propre parent ni l'ancêtre de son parent (« package P { package P { »)
    if e.parent == -1 and parent != -1 and not g.isAncestorOrSelf(idx, parent): e.parent = parent
    g.lastEntity = idx
  else:
    idx = g.addEntity(id, unescapeLabel(label), shape, line, parent, keyword)
  let e = g.entities[idx]
  if stereo.len > 0: e.stereo = stereo
  if color.len > 0: e.color = color
  if desc.len > 0:
    if isClassBody:
      if e.sections.len == 0: e.sections.add @[]
      for part in desc.split(';'):
        if part.strip().len > 0:
          e.sections[^1].add part.strip()
          e.noteLine(part.strip(), line)
    else:
      if e.sections.len == 0: e.sections.add @[]
      for part in splitLabel(desc):
        e.sections[0].add part
        e.noteLine(part, line)
  for (p, impl) in extends:
    let pi = g.ensure(p, p, (if impl: shClass else: shClass), line, parent)
    if impl and g.entities[pi].keyword == "": g.entities[pi].keyword = "interface"
    g.rels.add Rel(a: idx, b: pi, rightHead: "triangle", dashed: impl, minlen: 1, up: true, line: line)
  if opens:
    if isClassBody:
      bodyOf = idx
    else:
      e.isContainer = true
      if scope.len >= MaxNesting: tooDeep(line)
      scope.add idx
  true

proc addMember(e: Entity, raw: string, line: int) =
  let t = raw.strip()
  if t.len == 0: return
  var sepText = ""
  var isSep = false
  for s in ["--", "..", "==", "__"]:
    if t.startsWith(s) and t.endsWith(s):
      isSep = true
      sepText = t.strip(chars = {'-', '.', '=', '_'}).strip()
  if isSep:
    e.explicitSections = true
    if e.sections.len == 0: e.sections.add @[]
    e.sections.add (if sepText.len > 0: @[SectionMark & sepText] else: @[])
    if sepText.len > 0: e.noteLine(SectionMark & sepText, line)
    return
  if e.sections.len == 0: e.sections.add @[]
  e.sections[^1].add t
  e.noteLine(t, line)

proc parseGraph*(doc: Doc, ctx: Context): GraphDiagram =
  var g = GraphDiagram(kind: doc.kind, leftToRight: doc.meta.leftToRight, lastEntity: -1)
  var scope: seq[int]
  var bodyOf = -1
  var i = 0
  let lines = doc.lines
  while i < lines.len:
    let l = lines[i]
    let t = l.text.strip()
    inc i
    if t.len == 0: continue
    if bodyOf >= 0:
      if t == "}" or t.startsWith("}"):
        bodyOf = -1
      else:
        g.entities[bodyOf].addMember(t, l.num)
      continue
    let lw = t.toLowerAscii()
    if t == "}" or lw == "end package" or lw == "end state" or lw == "end node":
      if scope.len > 0: discard scope.pop()
      else: ctx.warn(l.num, "accolade fermante sans conteneur ouvert")
      continue
    if t in ["--", "||"]:
      ctx.info(l.num, "régions concurrentes affichées dans un seul état composite")
      continue
    if lw.startsWithWord("hide") or lw.startsWithWord("show"):
      let hide = lw.startsWithWord("hide")
      let what = t.afterWord(if hide: "hide" else: "show").toLowerAscii()
      if what.contains("circle"): g.hideCircle = hide
      elif what.contains("empty member") or what.contains("empty field") or what.contains("empty method") or what.contains("empty attribute"):
        g.hideEmpty = hide
      elif what.contains("stereotype"): g.hideStereo = hide
      elif what.contains("method"): g.hideMethods = hide
      elif what.contains("field") or what.contains("attribute"): g.hideFields = hide
      elif what.contains("member"): g.hideMembers = hide
      else: ctx.info(l.num, "directive hide/show ignorée : " & t)
      continue
    # --- notes
    if lw.startsWithWord("note") or lw.startsWithWord("hnote") or lw.startsWithWord("rnote"):
      var rest = t.afterWord(t.split({' ', '\t'})[0])
      var text = ""
      var target = -1
      var placement = "right"
      var alias = ""
      var color = ""
      (rest, color) = stripColor(rest)
      let colon = rest.find(':')
      var head = if colon >= 0: rest[0 ..< colon].strip() else: rest.strip()
      let inline = colon >= 0
      if inline: text = rest[colon + 1 .. ^1].strip()
      let hl = head.toLowerAscii()
      var onLink = false
      if hl.startsWith("\""):
        var sc = initScanner(head)
        text = sc.readQuoted()
        sc.skipWs()
        if sc.acceptWord("as"):
          sc.skipWs()
          alias = sc.readName()
      elif hl.startsWithWord("as"):
        alias = head.afterWord("as")
      elif hl.startsWith("on link"):
        onLink = true
      else:
        for p in ["left", "right", "top", "bottom", "over"]:
          if hl.startsWithWord(p):
            placement = p
            var tgt = head.afterWord(p)
            if tgt.toLowerAscii().startsWithWord("of"): tgt = tgt.afterWord("of")
            tgt = tgt.strip()
            if tgt.len > 0:
              var sc = initScanner(tgt)
              let r = parseRef(sc, g.kind)
              if r[4]:
                target = g.ensure(r[0], r[1], r[2], l.num, g.scopeOf(scope))
            elif g.lastEntity >= 0:
              target = g.lastEntity
            break
      if not inline and text.len == 0:
        var parts: seq[string]
        var closed = false
        while i < lines.len:
          let s = lines[i].text
          inc i
          if s.strip().toLowerAscii() in ["end note", "endnote", "end hnote", "endhnote", "end rnote", "endrnote"]:
            closed = true
            break
          parts.add s.strip()
        if not closed: ctx.error(l.num, "note sans « end note »")
        text = parts.join("\n")
      if onLink:
        if g.rels.len > 0:
          let r = g.rels.high
          g.rels[r].label = (if g.rels[r].label.len > 0: g.rels[r].label & "\n" else: "") & unescapeLabel(text)
        else: ctx.warn(l.num, "note on link sans relation précédente")
        continue
      let nid = if alias.len > 0: alias else: "__note" & $l.num
      let saveLast = g.lastEntity
      let parentScope = if target >= 0: g.entities[target].parent else: g.scopeOf(scope)
      let ni = g.addEntity(nid, "", shNote, l.num, parentScope)
      g.entities[ni].sections = @[splitLabel(text)]
      if color.len > 0: g.entities[ni].color = color
      g.lastEntity = saveLast
      if target >= 0:
        var r = Rel(a: ni, b: target, dashed: true, line: l.num)
        case placement
        of "left": r.minlen = 0
        of "right": (r.minlen = 0; r.a = target; r.b = ni)
        of "top": r.minlen = 1
        of "bottom", "over": (r.minlen = 1; r.a = target; r.b = ni)
        else: discard
        g.rels.add r
      continue
    # --- déclarations, relations, membres
    if g.parseDecl(t, l.num, scope, bodyOf, ctx): continue
    if g.tryRelation(t, l.num, g.scopeOf(scope), ctx): continue
    let colon = t.find(" : ")
    let colon2 = if colon < 0: t.find(':') else: colon
    if colon2 > 0:
      var sc = initScanner(t[0 ..< colon2].strip())
      let r = parseRef(sc, g.kind)
      sc.skipWs()
      if r[4] and sc.eof and r[3] == rkName:
        let idx = g.ensure(r[0], r[1], r[2], l.num, g.scopeOf(scope))
        let member = t[t.find(':', colon2) + 1 .. ^1].strip()
        let e = g.entities[idx]
        if e.shape in {shClass, shObject, shMap}:
          e.addMember(member, l.num)
        else:
          if e.sections.len == 0: e.sections.add @[]
          for part in splitLabel(member):
            e.sections[0].add part
            e.noteLine(part, l.num)
        continue
    block shorthand:
      var sc = initScanner(t)
      let r = parseRef(sc, g.kind)
      sc.skipWs()
      var alias = ""
      if sc.acceptWord("as"):
        sc.skipWs()
        alias = sc.readName()
        sc.skipWs()
      var (_, color) = stripColor(sc.rest)
      if r[4] and r[3] == rkName and (sc.eof or sc.peek in {'#', '<'}) and
          (r[2] != defaultShape(g.kind) or t.startsWith("\"") or alias.len > 0):
        let id = if alias.len > 0: alias else: r[0]
        let idx = g.ensure(id, r[1], r[2], l.num, g.scopeOf(scope))
        if color.len > 0: g.entities[idx].color = color
        let st = splitStereotype(sc.rest)[1]
        if st.len > 0: g.entities[idx].stereo = st
        g.lastEntity = idx
        continue
    ctx.warn(l.num, "syntaxe non reconnue, ligne ignorée : " & t)
  if bodyOf >= 0:
    ctx.error(g.entities[bodyOf].line, "corps de « " & g.entities[bodyOf].label & " » non fermé")
  if scope.len > 0:
    ctx.error(g.entities[scope[^1]].line, "conteneur « " & g.entities[scope[^1]].label & " » non fermé")
  g

# ------------------------------------------------------------------ tailles

proc isClassLike(e: Entity): bool = e.shape in {shClass, shObject, shMap}

proc memberText(m: string): (string, string, bool, bool) =
  ## (visibilité, texte, statique, abstrait)
  var t = m
  var st, ab = false
  var changed = true
  while changed:
    changed = false
    for (tag, flag) in [("{static}", 0), ("{classifier}", 0), ("{abstract}", 1), ("{field}", 2), ("{method}", 2)]:
      if t.startsWith(tag):
        t = t[tag.len .. ^1].strip()
        changed = true
        if flag == 0: st = true
        elif flag == 1: ab = true
  var vis = ""
  if t.len > 1 and t[0] in {'+', '-', '#', '~'} and t[1] != t[0]:
    vis = $t[0]
    t = t[1 .. ^1].strip()
  for (tag, flag) in [("{static}", 0), ("{abstract}", 1)]:
    if t.startsWith(tag):
      t = t[tag.len .. ^1].strip()
      if flag == 0: st = true else: ab = true
  (vis, unescapeLabel(t), st, ab)

proc classSections(g: GraphDiagram, e: Entity): seq[seq[string]] =
  if e.explicitSections or not e.isClassLike or e.shape != shClass:
    result = e.sections
  else:
    var fields, methods: seq[string]
    for s in e.sections:
      for m in s:
        let isMethod = (m.contains("(") and not m.startsWith("{field}")) or m.startsWith("{method}")
        if isMethod: methods.add m else: fields.add m
    if g.hideFields: fields = @[]
    if g.hideMethods: methods = @[]
    result = @[fields, methods]
    if g.hideEmpty:
      result = @[]
      if fields.len > 0: result.add fields
      if methods.len > 0: result.add methods
  if g.hideMembers and e.shape == shClass: result = @[]
  if e.shape in {shObject, shMap} and result.len == 0: result = @[@[]]

proc labelLines(e: Entity): seq[string] = splitLabel(e.label)

proc stereoText(g: GraphDiagram, e: Entity): string =
  if e.stereo.len == 0 or g.hideStereo: ""
  elif e.stereo.startsWith("$") and Sprites.hasKey(e.stereo[1 .. ^1]): "<" & e.stereo & ">"
  else: "«" & e.stereo & "»"

proc headerSize(g: GraphDiagram, e: Entity): (float, float) =
  let lines = e.labelLines
  var w = blockWidth(lines, FontSize, true)
  var h = blockHeight(lines.len)
  let st = g.stereoText(e)
  if st.len > 0:
    w = max(w, textWidth(st, 11))
    h += 14
  (w, h)

proc measure(g: GraphDiagram, e: Entity) =
  let (hw, hh) = g.headerSize(e)
  case e.shape
  of shClass, shObject, shMap:
    let icon = if e.shape == shClass and not g.hideCircle: 26.0 else: 0.0
    var w = hw + icon + 24
    var h = max(hh, 18.0) + 12
    for s in g.classSections(e):
      var sh = 8.0
      for m in s:
        if m.startsWith(SectionMark):
          w = max(w, textWidth(m[SectionMark.len .. ^1], 11) + 40)
          sh += 16
        else:
          let (vis, txt, _, _) = memberText(m)
          w = max(w, textWidth(txt) + (if vis.len > 0: 30.0 else: 20.0))
          sh += LineHeight
      h += max(sh, 10.0)
    e.w = max(w, 60)
    e.h = h
  of shUsecase:
    e.w = hw * 1.25 + 36
    e.h = hh * 1.3 + 26
  of shActor, shPerson:
    e.w = max(36.0, hw + 8)
    e.h = (if e.shape == shActor: 52.0 else: 40.0) + hh
    if e.shape == shPerson: e.w = max(e.w, hw + 30)
  of shInterface, shCircle:
    e.w = max(24.0, hw + 6)
    e.h = 22 + hh
  of shBoundary, shControl, shEntity:
    e.w = max(44.0, hw + 6)
    e.h = 34 + hh
  of shStart: (e.w = 22; e.h = 22)
  of shEnd: (e.w = 24; e.h = 24)
  of shPoint: (e.w = 10; e.h = 10)
  of shChoice, shDiamond: (e.w = 26; e.h = 26)
  of shForkBar:
    if g.leftToRight: (e.w = 8; e.h = 70) else: (e.w = 80; e.h = 8)
  of shHistory, shDeepHistory: (e.w = 26; e.h = 26)
  of shPort: (e.w = 14; e.h = 14 + (if e.label.len > 0: hh else: 0))
  of shNote:
    let lines = if e.sections.len > 0: e.sections[0] else: @[""]
    e.w = blockWidth(lines) + 26
    e.h = blockHeight(lines.len) + 14
  of shState:
    var w = hw + 30
    var h = hh + 14
    if e.sections.len > 0 and e.sections[0].len > 0:
      w = max(w, blockWidth(e.sections[0]) + 24)
      h += blockHeight(e.sections[0].len) + 10
    e.w = max(w, 60)
    e.h = max(h, 36)
  of shActivity:
    e.w = hw + 30
    e.h = hh + 18
  of shLabel:
    e.w = hw + 4
    e.h = hh + 4
  else:
    var extraW = 30.0
    var extraH = 22.0
    case e.shape
    of shComponent: extraW = 56
    of shNode: (extraW = 40; extraH = 32)
    of shDatabase: (extraW = 30; extraH = 40)
    of shCloud: (extraW = 50; extraH = 34)
    of shQueue: extraW = 50
    of shHexagon: extraW = 50
    of shFolder: extraH = 32
    of shCollections: (extraW = 36; extraH = 28)
    else: discard
    e.w = hw + extraW
    e.h = hh + extraH
  if e.isContainer:
    discard

proc ancestorAt(g: GraphDiagram, idx, level: int): int =
  ## Ancêtre de idx dont le parent est `level` (ou -1).
  var cur = idx
  while cur >= 0:
    if g.entities[cur].parent == level: return cur
    cur = g.entities[cur].parent
  -1

const ContPad = 14.0

proc containerTitleH(g: GraphDiagram, e: Entity): float =
  if e.shape == shTogether: 0.0
  else:
    let (_, hh) = g.headerSize(e)
    hh + 14 + (if e.shape == shNode: 8.0 else: 0.0) + (if e.shape == shDatabase: 12.0 else: 0.0) +
      (if e.shape == shState and e.sections.len > 0 and e.sections[0].len > 0: blockHeight(e.sections[0].len) + 6 else: 0.0)

proc layoutLevel(g: var GraphDiagram, level: int, edgeIdx: var seq[(int, int)]): (float, float) =
  var children: seq[int]
  for i, e in g.entities:
    if e.parent == level: children.add i
  for c in children:
    if g.entities[c].isContainer:
      let (cw, ch) = g.layoutLevel(c, edgeIdx)
      let e = g.entities[c]
      let pad = if e.shape == shTogether: 0.0 else: ContPad
      let tw = g.headerSize(e)[0] + 40
      e.contentW = cw
      e.contentH = ch
      e.w = max(cw + 2 * pad, tw)
      e.h = ch + 2 * pad + g.containerTitleH(e)
    else:
      g.measure(g.entities[c])
  var gr = initGraph(hgap = (if g.kind == dkState: 50.0 else: 45.0), vgap = 60.0)
  gr.leftToRight = g.leftToRight
  var local = initTable[int, int]()
  for c in children:
    local[c] = gr.addNode(g.entities[c].w, g.entities[c].h)
  var labelled = false
  for ri, r in g.rels:
    let ca = g.ancestorAt(r.a, level)
    let cb = g.ancestorAt(r.b, level)
    if ca < 0 or cb < 0 or ca == cb: continue
    if r.label.len > 0: labelled = true
    if r.up: gr.addEdge(local[cb], local[ca], r.minlen)
    else: gr.addEdge(local[ca], local[cb], r.minlen)
    if ca == r.a and cb == r.b:
      edgeIdx[ri] = (level, gr.edges.high)
  if labelled: gr.vgap = 75
  gr.layout()
  for c in children:
    let n = gr.nodes[local[c]]
    g.entities[c].relX = n.x
    g.entities[c].relY = n.y
  # points des arêtes directes (relatifs au niveau)
  for ri, r in g.rels:
    if edgeIdx[ri][0] == level and edgeIdx[ri][1] >= 0:
      var pts = gr.edges[edgeIdx[ri][1]].points
      if r.up:
        var rev: seq[(float, float)]
        for k in countdown(pts.high, 0): rev.add pts[k]
        pts = rev
      g.rels[ri].points = pts
  (gr.width, gr.height)

proc placeAbs(g: var GraphDiagram, level: int, ox, oy: float) =
  for i, e in g.entities:
    if e.parent == level:
      e.x = ox + e.relX
      e.y = oy + e.relY
      if e.isContainer:
        let pad = if e.shape == shTogether: 0.0 else: ContPad
        let innerX = e.x + (e.w - e.contentW) / 2
        let innerY = e.y + g.containerTitleH(e) + pad
        g.placeAbs(i, innerX, innerY)

proc levelOrigin(g: GraphDiagram, level: int): (float, float) =
  if level < 0: return (0.0, 0.0)
  let e = g.entities[level]
  let pad = if e.shape == shTogether: 0.0 else: ContPad
  (e.x + (e.w - e.contentW) / 2, e.y + g.containerTitleH(e) + pad)

# ------------------------------------------------------------------ dessin

proc center(e: Entity): (float, float) = (e.x + e.w / 2, e.y + e.h / 2)

proc isRound(e: Entity): bool =
  e.shape in {shUsecase, shStart, shEnd, shInterface, shCircle, shHistory, shDeepHistory, shPoint, shPort}

proc shapeBox(e: Entity): (float, float, float, float) =
  ## Boîte utile pour le rognage (formes avec libellé sous l'icône).
  case e.shape
  of shInterface, shCircle: (e.x + e.w / 2 - 9, e.y, 18.0, 18.0)
  of shActor: (e.x + e.w / 2 - 14, e.y, 28.0, 46.0)
  of shBoundary, shControl, shEntity: (e.x + e.w / 2 - 20, e.y, 40.0, 30.0)
  of shPort: (e.x + e.w / 2 - 7, e.y, 14.0, 14.0)
  else: (e.x, e.y, e.w, e.h)

proc clip(e: Entity, fromP, toP: (float, float)): (float, float) =
  ## Point d'intersection du segment toP → fromP (fromP intérieur) avec le bord.
  let (bx, by, bw, bh) = e.shapeBox
  # arête décalée (parallèle à une autre) : suivre son propre trait, sinon les
  # deux arêtes convergeraient vers le centre de la boîte
  let inside = fromP[0] > bx and fromP[0] < bx + bw and fromP[1] > by and fromP[1] < by + bh
  if inside and not e.isRound and e.shape notin {shChoice, shDiamond}:
    let (ux, uy) = (toP[0] - fromP[0], toP[1] - fromP[1])
    let tx = if ux > 1e-6: (bx + bw - fromP[0]) / ux elif ux < -1e-6: (bx - fromP[0]) / ux else: Inf
    let ty = if uy > 1e-6: (by + bh - fromP[1]) / uy elif uy < -1e-6: (by - fromP[1]) / uy else: Inf
    let t = min(tx, ty)
    if t != Inf: return (fromP[0] + ux * t, fromP[1] + uy * t)
  let cx = bx + bw / 2
  let cy = by + bh / 2
  var dx = toP[0] - cx
  var dy = toP[1] - cy
  if abs(dx) < 1e-6 and abs(dy) < 1e-6: return (cx, cy)
  if e.isRound:
    let rx = bw / 2
    let ry = bh / 2
    let t = 1.0 / sqrt((dx * dx) / (rx * rx) + (dy * dy) / (ry * ry))
    return (cx + dx * t, cy + dy * t)
  if e.shape in {shChoice, shDiamond}:
    let t = 1.0 / (abs(dx) / (bw / 2) + abs(dy) / (bh / 2))
    return (cx + dx * t, cy + dy * t)
  let tx = if abs(dx) > 1e-6: (bw / 2) / abs(dx) else: Inf
  let ty = if abs(dy) > 1e-6: (bh / 2) / abs(dy) else: Inf
  let t = min(tx, ty)
  (cx + dx * t, cy + dy * t)

proc visIcon(svg: var Svg, x, y: float, vis: string, isMethod: bool) =
  ## Petite icône de visibilité centrée en (x, y).
  let (col, fillc) = case vis
    of "+": ("#2E7D32", "#81C784")
    of "-": ("#C62828", "#EF9A9A")
    of "#": ("#B26A00", "#FFD54F")
    else: ("#1565C0", "#90CAF9")
  let f = if isMethod: fillc else: "none"
  case vis
  of "+": svg.circle(x, y, 3.6, f, col)
  of "-": svg.rect(x - 3.4, y - 3.4, 6.8, 6.8, f, col)
  of "#": svg.polygon([(x, y - 4.5), (x + 4.5, y), (x, y + 4.5), (x - 4.5, y)], f, col)
  else: svg.polygon([(x, y - 4), (x + 4.5, y + 3.5), (x - 4.5, y + 3.5)], f, col)

proc classIcon(e: Entity): (string, string) =
  case e.keyword
  of "interface": ("I", "#B4A7E5")
  of "enum": ("E", "#EB937F")
  of "abstract", "abstract class": ("A", "#A9DCDF")
  of "annotation": ("@", "#E3664A")
  of "entity": ("E", "#ADD1B2")
  of "exception": ("X", "#E3664A")
  of "struct", "record", "dataclass": ("S", "#ADD1B2")
  of "protocol": ("P", "#B4A7E5")
  else: ("C", "#ADD1B2")

proc drawLabelUnder(svg: var Svg, e: Entity, top: float) =
  svg.textBlock(e.x + e.w / 2, top, e.labelLines, "middle")

proc styleKey(e: Entity): string =
  case e.shape
  of shClass: "class"
  of shObject: "object"
  of shMap: "map"
  of shUsecase: "usecase"
  of shActor, shPerson: "actor"
  of shComponent: "component"
  of shInterface, shCircle: "interface"
  of shNode: "node"
  of shDatabase: "database"
  of shCloud: "cloud"
  of shArtifact: "artifact"
  of shFile: "file"
  of shFolder: "folder"
  of shFrame: "frame"
  of shPackage: "package"
  of shRectangle: (if e.keyword == "archimate": "archimate" else: "rectangle")
  of shQueue: "queue"
  of shStack: "stack"
  of shCard: "card"
  of shAgent: "agent"
  of shStorage: "storage"
  of shState: "state"
  of shActivity, shForkBar: "activity"
  of shBoundary: "boundary"
  of shControl: "control"
  of shEntity: "entity"
  of shCollections: "collections"
  of shNote: "note"
  else: ""

proc drawEntityStyled(g: GraphDiagram, svg: var Svg, e: Entity, ctx: Context)

proc drawEntity(g: GraphDiagram, svg: var Svg, e: Entity, ctx: Context) =
  styled(styleKey(e)):
    g.drawEntityStyled(svg, e, ctx)

proc drawEntityStyled(g: GraphDiagram, svg: var Svg, e: Entity, ctx: Context) =
  let fill = if e.color.len > 0: e.color else: Fill
  let (x, y, w, h) = (e.x, e.y, e.w, e.h)
  let st = g.stereoText(e)
  ctx.region(e.line, x, y, w, h)
  # chaque membre renvoie à sa ligne : le plus petit élément sous le pointeur l'emporte
  var memberSeen = initCountTable[string]()
  proc memberRegion(member: string, rx, ry, rw, rh: float) =
    let lines = e.memberLines.getOrDefault(member)
    let index = memberSeen[member]
    memberSeen.inc(member)
    if lines.len > 0: ctx.region(lines[min(index, lines.high)], rx, ry, rw, rh)
  proc header(svg: var Svg, top: float, bold = true, italic = false, underline = false, center = -1.0) =
    let c = if center >= 0: center else: x + w / 2
    var yy = top
    if st.len > 0:
      svg.text(c, yy + 11, st, "middle", 11, italic = true, fill = Muted)
      yy += 14
    let lines = e.labelLines
    for i, l in lines:
      svg.text(c, yy + 13 + float(i) * LineHeight, l, "middle", FontSize, bold, italic, Ink, underline)
  case e.shape
  of shClass, shObject, shMap:
    svg.rect(x, y, w, h, fill, Stroke, 3)
    let (hw, hh) = g.headerSize(e)
    let headH = max(hh, 18.0) + 12
    let icon = e.shape == shClass and not g.hideCircle
    let italic = e.keyword in ["abstract", "abstract class", "interface"]
    if icon:
      let (letter, col) = classIcon(e)
      let total = hw + 26
      let ix = x + (w - total) / 2 + 9
      svg.circle(ix, y + headH / 2, 9, col, Stroke, 1)
      svg.text(ix, y + headH / 2 + 4.5, letter, "middle", 12, bold = true)
      var yy = y + (headH - hh) / 2
      let tx = ix + 17 + hw / 2
      if st.len > 0:
        svg.text(tx, yy + 11, st, "middle", 11, italic = true, fill = Muted)
        yy += 14
      for i, l in e.labelLines:
        svg.text(tx, yy + 13 + float(i) * LineHeight, l, "middle", FontSize, true, italic)
    else:
      header(svg, y + (headH - hh) / 2, bold = e.shape == shClass, underline = e.shape == shObject)
    var yy = y + headH
    for s in g.classSections(e):
      svg.line(x, yy, x + w, yy)
      var sh = 8.0
      var ly = yy + 4
      for m in s:
        if m.startsWith(SectionMark):
          svg.text(x + w / 2, ly + 12, m[SectionMark.len .. ^1], "middle", 11, bold = true, fill = Muted)
          memberRegion(m, x + 1, ly, w - 2, 16)
          ly += 16
          sh += 16
          continue
        let (vis, txt, isStatic, isAbs) = memberText(m)
        var tx = x + 8
        if vis.len > 0:
          svg.visIcon(x + 12, ly + LineHeight / 2, vis, txt.contains("("))
          tx = x + 22
        if e.shape == shMap and txt.contains("=>"):
          let parts = txt.split("=>", 1)
          svg.text(tx, ly + 12, parts[0].strip(), bold = true)
          svg.text(tx + textWidth(parts[0].strip(), FontSize, true) + 10, ly + 12, parts[1].strip())
        else:
          svg.text(tx, ly + 12, txt, italic = isAbs, underline = isStatic)
        memberRegion(m, x + 1, ly, w - 2, LineHeight)
        ly += LineHeight
        sh += LineHeight
      yy += max(sh, 10.0)
  of shUsecase:
    svg.ellipse(x + w / 2, y + h / 2, w / 2, h / 2, fill)
    let (_, hh) = g.headerSize(e)
    header(svg, y + (h - hh) / 2, bold = false)
  of shActor:
    let cx = x + w / 2
    svg.circle(cx, y + 8, 7.5, fill)
    svg.line(cx, y + 15.5, cx, y + 32)
    svg.line(cx - 13, y + 21, cx + 13, y + 21)
    svg.line(cx, y + 32, cx - 11, y + 46)
    svg.line(cx, y + 32, cx + 11, y + 46)
    header(svg, y + 50, bold = false)
  of shPerson:
    let cx = x + w / 2
    svg.circle(cx, y + 9, 9, fill)
    svg.rect(x, y + 18, w, h - 18, fill, Stroke, 10)
    header(svg, y + 22)
  of shInterface, shCircle:
    svg.circle(x + w / 2, y + 9, 9, fill)
    header(svg, y + 20, bold = false)
  of shBoundary:
    let cx = x + w / 2
    svg.line(cx - 18, y + 4, cx - 18, y + 26)
    svg.line(cx - 18, y + 15, cx - 10, y + 15)
    svg.circle(cx + 2, y + 15, 12, fill)
    header(svg, y + 32, bold = false)
  of shControl:
    let cx = x + w / 2
    svg.circle(cx, y + 16, 12, fill)
    svg.arrowHead(cx + 3, y + 4, PI, "open")
    header(svg, y + 32, bold = false)
  of shEntity:
    let cx = x + w / 2
    svg.circle(cx, y + 14, 12, fill)
    svg.line(cx - 12, y + 28, cx + 12, y + 28)
    header(svg, y + 32, bold = false)
  of shStart:
    svg.circle(x + w / 2, y + h / 2, 10, Ink, Ink)
  of shEnd:
    svg.circle(x + w / 2, y + h / 2, 11, Background, Ink, 1.4)
    svg.circle(x + w / 2, y + h / 2, 6.5, Ink, Ink)
  of shPoint:
    svg.circle(x + w / 2, y + h / 2, 4, Ink, Ink)
  of shChoice, shDiamond:
    svg.polygon([(x + w / 2, y), (x + w, y + h / 2), (x + w / 2, y + h), (x, y + h / 2)], fill)
  of shForkBar:
    svg.rect(x, y, w, h, Ink, Ink, 2)
  of shHistory, shDeepHistory:
    svg.circle(x + w / 2, y + h / 2, 12, fill)
    svg.text(x + w / 2, y + h / 2 + 4.5, (if e.shape == shHistory: "H" else: "H*"), "middle", 12, bold = true)
  of shPort:
    svg.rect(x + w / 2 - 7, y, 14, 14, fill)
    if e.label.len > 0: header(svg, y + 15, bold = false)
  of shNote:
    svg.note(x, y, w, h, if e.color.len > 0: e.color else: NoteFill)
    if e.sections.len > 0: svg.textBlock(x + 10, y + 7, e.sections[0])
  of shState:
    if e.isContainer:
      svg.rect(x, y, w, h, FrameFill, Stroke, 12)
      let th = g.containerTitleH(e)
      header(svg, y + 6)
      if e.sections.len > 0 and e.sections[0].len > 0:
        svg.textBlock(x + 10, y + 6 + g.headerSize(e)[1] + 4, e.sections[0], size = 12)
        for i, part in e.sections[0]:
          memberRegion(part, x + 1, y + 10 + g.headerSize(e)[1] + blockHeight(i, 12), w - 2, blockHeight(1, 12))
      svg.line(x, y + th, x + w, y + th)
    else:
      svg.rect(x, y, w, h, fill, Stroke, 12)
      let (_, hh) = g.headerSize(e)
      let hasDesc = e.sections.len > 0 and e.sections[0].len > 0
      if hasDesc:
        header(svg, y + 7)
        let sy = y + hh + 12
        svg.line(x, sy, x + w, sy)
        svg.textBlock(x + 10, sy + 5, e.sections[0])
        for i, part in e.sections[0]:
          memberRegion(part, x + 1, sy + 5 + blockHeight(i), w - 2, blockHeight(1))
      else:
        header(svg, y + (h - hh) / 2)
  of shActivity:
    svg.rect(x, y, w, h, fill, Stroke, 12)
    let (_, hh) = g.headerSize(e)
    header(svg, y + (h - hh) / 2, bold = false)
  of shLabel:
    header(svg, y + 2, bold = false)
  of shTogether:
    discard
  else:
    # formes de type boîte, conteneurs inclus
    let cont = e.isContainer
    let (_, hh) = g.headerSize(e)
    let titleTop = y + 7
    let bg = if cont and e.color.len == 0: FrameFill else: fill
    case e.shape
    of shPackage, shFolder:
      let tabW = if cont: min(w - 8, g.headerSize(e)[0] + 24) else: min(w * 0.6, g.headerSize(e)[0] + 24)
      let tabH = if cont: hh + 8 else: 12.0
      if cont:
        svg.path("M" & fmtF(x) & " " & fmtF(y + tabH) & " V" & fmtF(y) & " H" & fmtF(x + tabW) &
          " L" & fmtF(x + tabW + 8) & " " & fmtF(y + tabH), bg)
        svg.rect(x, y + tabH, w, h - tabH, bg, Stroke)
        header(svg, y + 4, center = x + tabW / 2)
      else:
        svg.rect(x, y, tabW, tabH, bg, Stroke)
        svg.rect(x, y + tabH, w, h - tabH, bg, Stroke)
        header(svg, y + tabH + (h - tabH - hh) / 2)
    of shNode:
      let d = 8.0
      svg.polygon([(x, y + d), (x + d, y), (x + w, y), (x + w, y + h - d), (x + w - d, y + h), (x, y + h)], bg)
      svg.path("M" & fmtF(x) & " " & fmtF(y + d) & " H" & fmtF(x + w - d) & " L" & fmtF(x + w) & " " & fmtF(y) &
        " M" & fmtF(x + w - d) & " " & fmtF(y + d) & " V" & fmtF(y + h))
      header(svg, if cont: y + d + 6 else: y + d + (h - d - hh) / 2)
    of shDatabase:
      let ry = 8.0
      svg.path("M" & fmtF(x) & " " & fmtF(y + ry) & " A" & fmtF(w / 2) & " " & fmtF(ry) & " 0 0 1 " &
        fmtF(x + w) & " " & fmtF(y + ry) & " V" & fmtF(y + h - ry) & " A" & fmtF(w / 2) & " " & fmtF(ry) &
        " 0 0 1 " & fmtF(x) & " " & fmtF(y + h - ry) & " Z", bg)
      svg.path("M" & fmtF(x) & " " & fmtF(y + ry) & " A" & fmtF(w / 2) & " " & fmtF(ry) & " 0 0 0 " &
        fmtF(x + w) & " " & fmtF(y + ry))
      header(svg, if cont: y + 2 * ry + 6 else: y + 2 * ry + (h - 3 * ry - hh) / 2)
    of shCloud:
      svg.rect(x, y, w, h, bg, Stroke, min(h / 2, 28.0))
      header(svg, if cont: titleTop + 4 else: y + (h - hh) / 2)
    of shArtifact, shFile:
      let f = 10.0
      svg.path("M" & fmtF(x) & " " & fmtF(y) & " H" & fmtF(x + w - f) & " L" & fmtF(x + w) & " " &
        fmtF(y + f) & " V" & fmtF(y + h) & " H" & fmtF(x) & " Z", bg)
      svg.path("M" & fmtF(x + w - f) & " " & fmtF(y) & " V" & fmtF(y + f) & " H" & fmtF(x + w))
      header(svg, if cont: titleTop else: y + (h - hh) / 2, bold = false)
    of shFrame:
      svg.rect(x, y, w, h, bg)
      let tw = g.headerSize(e)[0] + 18
      svg.path("M" & fmtF(x) & " " & fmtF(y + hh + 8) & " H" & fmtF(x + tw) & " L" & fmtF(x + tw + 8) & " " &
        fmtF(y + hh) & " V" & fmtF(y))
      svg.textBlock(x + 8, y + 4, e.labelLines, bold = true)
    of shQueue:
      let rx = 8.0
      svg.path("M" & fmtF(x + rx) & " " & fmtF(y) & " H" & fmtF(x + w - rx) & " A" & fmtF(rx) & " " & fmtF(h / 2) &
        " 0 0 1 " & fmtF(x + w - rx) & " " & fmtF(y + h) & " H" & fmtF(x + rx) & " A" & fmtF(rx) & " " &
        fmtF(h / 2) & " 0 0 1 " & fmtF(x + rx) & " " & fmtF(y) & " Z", bg)
      svg.path("M" & fmtF(x + w - rx) & " " & fmtF(y) & " A" & fmtF(rx) & " " & fmtF(h / 2) & " 0 0 0 " &
        fmtF(x + w - rx) & " " & fmtF(y + h))
      header(svg, if cont: titleTop else: y + (h - hh) / 2)
    of shHexagon:
      let d = 14.0
      svg.polygon([(x + d, y), (x + w - d, y), (x + w, y + h / 2), (x + w - d, y + h), (x + d, y + h), (x, y + h / 2)], bg)
      header(svg, if cont: titleTop else: y + (h - hh) / 2)
    of shStorage, shAgent, shCard, shStack:
      svg.rect(x, y, w, h, bg, Stroke, (if e.shape == shStorage: 14.0 else: 2.0))
      if e.shape == shCard: svg.line(x, y + hh + 12, x + w, y + hh + 12)
      header(svg, if cont: titleTop else: y + (h - hh) / 2)
    of shCollections:
      svg.rect(x + 5, y, w - 5, h - 5, bg)
      svg.rect(x, y + 5, w - 5, h - 5, bg)
      header(svg, y + 5 + (h - 5 - hh) / 2)
    of shComponent:
      svg.rect(x, y, w, h, bg, Stroke, 2)
      let ix = x + w - 22
      svg.rect(ix, y + 6, 15, 17, bg)
      svg.rect(ix - 4, y + 9, 8, 4, bg)
      svg.rect(ix - 4, y + 16, 8, 4, bg)
      header(svg, if cont: titleTop else: y + (h - hh) / 2, center = x + (w - 22) / 2)
    else:
      svg.rect(x, y, w, h, bg, Stroke, 2)
      header(svg, if cont: titleTop else: y + (h - hh) / 2)

type LabelBox = tuple[x, y, w, h: float]

proc overlaps(a, b: LabelBox): bool =
  a.x < b.x + b.w and b.x < a.x + a.w and a.y < b.y + b.h and b.y < a.y + a.h

proc isFree(g: GraphDiagram, box: LabelBox, placed: seq[LabelBox]): bool =
  ## Ni sur un autre libellé, ni sur une boîte (un conteneur peut contenir le libellé).
  for other in placed:
    if overlaps(box, other): return false
  for e in g.entities:
    if not e.isContainer and overlaps(box, (e.x, e.y, e.w, e.h)): return false
  true

proc pointAlong(pts: seq[(float, float)], fraction: float): (float, float, bool) =
  ## Point à cette fraction de la longueur du tracé, et si le trait y est plutôt vertical.
  var total = 0.0
  for k in 1 .. pts.high: total += hypot(pts[k][0] - pts[k - 1][0], pts[k][1] - pts[k - 1][1])
  var left = total * fraction
  for k in 1 .. pts.high:
    let (ax, ay) = pts[k - 1]
    let (bx, by) = pts[k]
    let d = hypot(bx - ax, by - ay)
    if d >= left or k == pts.high:
      let t = if d > 0: min(left / d, 1.0) else: 0.0
      return (ax + (bx - ax) * t, ay + (by - ay) * t, abs(by - ay) > abs(bx - ax))
    left -= d

proc endLabel(g: GraphDiagram, svg: var Svg, ctx: Context, placed: var seq[LabelBox], line: int, p, q: (float, float), s: string) =
  if s.len == 0: return
  let dx = q[0] - p[0]
  let dy = q[1] - p[1]
  let L = max(1e-6, sqrt(dx * dx + dy * dy))
  let ux = dx / L
  let uy = dy / L
  let tw = textWidth(s, 11) + 4
  # d'un côté du trait, ou de l'autre si un libellé y est déjà
  var px = p[0] + ux * 16 - uy * 12
  var py = p[1] + uy * 16 + ux * 12
  if not g.isFree((px - tw / 2, py - 8, tw, 15.0), placed):
    let (ox, oy) = (p[0] + ux * 16 + uy * 12, p[1] + uy * 16 - ux * 12)
    if g.isFree((ox - tw / 2, oy - 8, tw, 15.0), placed): (px, py) = (ox, oy)
  placed.add (px - tw / 2, py - 8, tw, 15.0)
  svg.text(px, py + 4, s, "middle", 11, fill = Muted)
  ctx.region(line, px - tw / 2, py - 8, tw, 15)

proc drawRel(g: GraphDiagram, svg, labels: var Svg, r: Rel, offset: float, ctx: Context, placed: var seq[LabelBox]) =
  if r.hidden: return
  let ea = g.entities[r.a]
  let eb = g.entities[r.b]
  let color = if r.color.len > 0: r.color else: arrowStroke()
  let sw = if r.bold: 2.2 else: LineWidth
  let dash = if r.dashed: "6 4" else: ""
  if r.a == r.b:
    let (bx, by, bw, bh) = ea.shapeBox
    let x0 = bx + bw
    let y0 = by + bh * 0.3
    let y1 = by + bh * 0.7
    svg.beginLink(ctx, r.line)
    svg.path("M" & fmtF(x0) & " " & fmtF(y0) & " C" & fmtF(x0 + 40) & " " & fmtF(y0 - 10) & " " &
      fmtF(x0 + 40) & " " & fmtF(y1 + 10) & " " & fmtF(x0) & " " & fmtF(y1), "none", color, sw, dash)
    if r.rightHead.len > 0: svg.arrowHead(x0, y1, PI + 0.3, r.rightHead, color)
    ctx.linkCubic(r.line, (x0, y0), (x0 + 40, y0 - 10), (x0 + 40, y1 + 10), (x0, y1))
    svg.endLink(r.line)
    if r.label.len > 0:
      let lines = r.label.split('\n')
      labels.textBlock(x0 + 34, y0 + (y1 - y0) / 2 - blockHeight(lines.len) / 2, lines, size = 12)
    if r.label.len > 0:
      let lines = r.label.split('\n')
      ctx.region(r.line, x0 + 34, y0 + (y1 - y0) / 2 - blockHeight(lines.len) / 2, blockWidth(lines, 12), blockHeight(lines.len))
      placed.add (x0 + 34, y0 + (y1 - y0) / 2 - blockHeight(lines.len) / 2, blockWidth(lines, 12), blockHeight(lines.len))
    return
  var ca = ea.center
  var cb = eb.center
  # décalage des arêtes parallèles
  let dx0 = cb[0] - ca[0]
  let dy0 = cb[1] - ca[1]
  let len0 = max(1e-6, sqrt(dx0 * dx0 + dy0 * dy0))
  let nx = -dy0 / len0 * offset
  let ny = dx0 / len0 * offset
  var pts: seq[(float, float)] = @[(ca[0] + nx, ca[1] + ny)]
  for p in r.points: pts.add (p[0] + nx, p[1] + ny)
  pts.add (cb[0] + nx, cb[1] + ny)
  # rognage aux bords (en partant du centre décalé)
  let sa = clip(ea, pts[0], pts[1])
  let sb = clip(eb, pts[^1], pts[^2])
  pts[0] = (sa[0] + nx * 0.0, sa[1])
  pts[0] = sa
  pts[^1] = sb
  # segments droits ; on retire les points presque alignés pour éviter les coudes inutiles
  var k = 1
  while k < pts.high:
    let (ax, ay) = pts[k - 1]
    let (bx, by) = pts[k + 1]
    let L = max(1e-6, hypot(bx - ax, by - ay))
    let dist = abs((bx - ax) * (ay - pts[k][1]) - (ax - pts[k][0]) * (by - ay)) / L
    if dist < 6: pts.delete(k) else: inc k
  var d = "M" & fmtF(pts[0][0]) & " " & fmtF(pts[0][1])
  for k in 1 .. pts.high:
    d.add " L" & fmtF(pts[k][0]) & " " & fmtF(pts[k][1])
  svg.beginLink(ctx, r.line)
  svg.path(d, "none", color, sw, dash)
  if r.leftHead.len > 0:
    let ang = arctan2(pts[0][1] - pts[1][1], pts[0][0] - pts[1][0])
    svg.arrowHead(pts[0][0], pts[0][1], ang, r.leftHead, color)
  if r.rightHead.len > 0:
    let ang = arctan2(pts[^1][1] - pts[^2][1], pts[^1][0] - pts[^2][0])
    svg.arrowHead(pts[^1][0], pts[^1][1], ang, r.rightHead, color)
  ctx.link(r.line, pts)
  svg.endLink(r.line)
  # libellé au milieu
  let mid = pts.len div 2
  let (mx, my) = if pts.len mod 2 == 0:
      ((pts[mid - 1][0] + pts[mid][0]) / 2, (pts[mid - 1][1] + pts[mid][1]) / 2)
    else: pts[mid]
  if r.label.len > 0:
    let lines = r.label.split('\n')
    let lw = blockWidth(lines, 12)
    let lh = blockHeight(lines.len, 12)
    # arêtes parallèles : libellé décalé du côté de l'arête
    # segment central plutôt vertical : libellé à côté du trait, pas dessus
    let k = max(1, pts.len div 2)
    let vertical = abs(pts[k][1] - pts[k - 1][1]) > abs(pts[k][0] - pts[k - 1][0])
    let side = if nx < -0.5: -1.0 else: 1.0  # arête parallèle : du côté où elle est décalée
    var lx = if nx < -0.5: mx - lw / 2 - 6
             elif nx > 0.5 or vertical: mx + lw / 2 + 6
             else: mx
    var ly = my
    # si cette place est prise, le libellé glisse le long de sa flèche, d'un côté puis de l'autre
    proc box(cx, cy: float): LabelBox = (cx - lw / 2 - 3, cy - lh / 2 - 1, lw + 6, lh + 2)
    if not g.isFree(box(lx, ly), placed):
      block search:
        # d'abord collé au trait ; dans un faisceau serré, un peu plus loin plutôt que par-dessus
        for extra in [0.0, 24.0, 56.0, 96.0]:
          for fraction in [0.5, 0.38, 0.62, 0.28, 0.72, 0.2, 0.8]:
            let (px, py, steep) = pointAlong(pts, fraction)
            let options =
              if steep: @[(px + side * (lw / 2 + 6 + extra), py), (px - side * (lw / 2 + 6 + extra), py)]
              else: @[(px, py - lh / 2 - 4 - extra), (px, py + lh / 2 + 4 + extra)]
            for (cx, cy) in options:
              if g.isFree(box(cx, cy), placed):
                (lx, ly) = (cx, cy)
                break search
    placed.add box(lx, ly)
    labels.rect(lx - lw / 2 - 3, ly - lh / 2 - 1, lw + 6, lh + 2, Background, "none", 0, 0, "", "fill-opacity=\"0.85\"")
    labels.textBlock(lx, ly - lh / 2, lines, "middle", 12)
    ctx.region(r.line, lx - lw / 2 - 3, ly - lh / 2 - 1, lw + 6, lh + 2)
  g.endLabel(labels, ctx, placed, r.line, pts[0], pts[1], r.multA)
  g.endLabel(labels, ctx, placed, r.line, pts[^1], pts[^2], r.multB)

proc segHits(p, q: (float, float), x, y, w, h: float): bool =
  ## Le segment p→q traverse-t-il le rectangle (Liang–Barsky) ?
  # le cas courant, de loin : le segment passe à côté, sans division
  if max(p[0], q[0]) < x or min(p[0], q[0]) > x + w or max(p[1], q[1]) < y or min(p[1], q[1]) > y + h:
    return false
  var t0 = 0.0
  var t1 = 1.0
  let dx = q[0] - p[0]
  let dy = q[1] - p[1]
  for (pp, qq) in [(-dx, p[0] - x), (dx, x + w - p[0]), (-dy, p[1] - y), (dy, y + h - p[1])]:
    if abs(pp) < 1e-9:
      if qq < 0: return false
    else:
      let r = qq / pp
      if pp < 0: t0 = max(t0, r)
      else: t1 = min(t1, r)
      if t0 > t1: return false
  true

proc isAncestor(g: GraphDiagram, anc, idx: int): bool =
  var cur = g.entities[idx].parent
  while cur >= 0:
    if cur == anc: return true
    cur = g.entities[cur].parent
  false

proc avoidObstacles(g: var GraphDiagram) =
  ## Arêtes droites traversant une autre boîte : détour par un point voisin.
  for ri in 0 ..< g.rels.len:
    let r = g.rels[ri]
    if r.points.len > 0 or r.a == r.b or r.hidden: continue
    let pa = g.entities[r.a].center
    let pb = g.entities[r.b].center
    proc blocked(g: GraphDiagram, p, q: (float, float), r: Rel): int =
      for i, e in g.entities:
        if i == r.a or i == r.b or e.isContainer or e.shape == shTogether: continue
        if g.isAncestor(i, r.a) or g.isAncestor(i, r.b): continue
        if segHits(p, q, e.x - 4, e.y - 4, e.w + 8, e.h + 8): return i
      -1
    let hit = g.blocked(pa, pb, r)
    if hit < 0: continue
    let o = g.entities[hit]
    var best: seq[(float, float)]
    var bestLen = Inf
    let m = 18.0
    let cands = @[(o.x - m, o.y + o.h / 2), (o.x + o.w + m, o.y + o.h / 2),
                  (o.x + o.w / 2, o.y - m), (o.x + o.w / 2, o.y + o.h + m),
                  (o.x - m, o.y - m), (o.x + o.w + m, o.y - m),
                  (o.x - m, o.y + o.h + m), (o.x + o.w + m, o.y + o.h + m)]
    proc tryPath(g: GraphDiagram, path: seq[(float, float)], best: var seq[(float, float)], bestLen: var float) =
      var prev = pa
      var L = 0.0
      for p in path & @[pb]:
        if g.blocked(prev, p, r) >= 0: return
        L += hypot(p[0] - prev[0], p[1] - prev[1])
        prev = p
      if L < bestLen:
        bestLen = L
        best = path
    for c in cands: g.tryPath(@[c], best, bestLen)
    if best.len == 0:
      for c1 in cands:
        for c2 in cands:
          if c1 != c2: g.tryPath(@[c1, c2], best, bestLen)
    if best.len > 0: g.rels[ri].points = best

proc paintAll(g: var GraphDiagram, ctx: Context): Svg =
  var svg: Svg
  # conteneurs d'abord (du plus externe au plus interne), puis entités
  proc depth(g: GraphDiagram, i: int): int =
    var cur = g.entities[i].parent
    while cur >= 0:
      inc result
      cur = g.entities[cur].parent
  var conts: seq[(int, int)]
  for i, e in g.entities:
    if e.isContainer: conts.add (g.depth(i), i)
  conts.sort()
  for (_, i) in conts: g.drawEntity(svg, g.entities[i], ctx)
  var labels: Svg
  var placed: seq[LabelBox]
  var pairCount = initTable[(int, int), int]()
  var pairSeen = initTable[(int, int), int]()
  for r in g.rels:
    let k = (min(r.a, r.b), max(r.a, r.b))
    pairCount[k] = pairCount.getOrDefault(k) + 1
  for r in g.rels:
    let k = (min(r.a, r.b), max(r.a, r.b))
    let n = pairCount[k]
    let idx = pairSeen.getOrDefault(k)
    pairSeen[k] = idx + 1
    # écart des arêtes parallèles : franc, mais borné par la plus petite boîte
    let ea = g.entities[r.a]
    let eb = g.entities[r.b]
    let room = min(min(ea.w, eb.w), min(ea.h, eb.h) * 3) * 0.7
    let step = if n > 1: min(44.0, room / float(n - 1)) else: 0.0
    var off = (float(idx) - float(n - 1) / 2) * step
    if r.a > r.b: off = -off
    # arête déjà routée par des points intermédiaires : pas de décalage supplémentaire
    if r.points.len > 0: off = 0
    g.drawRel(svg, labels, r, off, ctx, placed)
  for e in g.entities:
    if not e.isContainer: g.drawEntity(svg, e, ctx)
  # libellés après les boîtes pour rester lisibles
  svg.body.add labels.body
  svg

proc renderGraph*(g: var GraphDiagram, ctx: Context): (Svg, float, float) =
  if g.entities.len == 0:
    var empty: Svg
    return (empty, 0.0, 0.0)
  var edgeIdx = newSeq[(int, int)](g.rels.len)
  for i in 0 ..< edgeIdx.len: edgeIdx[i] = (-2, -1)
  let (w, h) = g.layoutLevel(-1, edgeIdx)
  let margin = 20.0
  g.placeAbs(-1, margin, margin)
  # points relatifs → absolus
  for ri in 0 ..< g.rels.len:
    let lvl = edgeIdx[ri][0]
    if lvl == -2: continue
    let (ox, oy) = g.levelOrigin(lvl)
    let o = if lvl == -1: (margin, margin) else: (ox, oy)
    for k in 0 ..< g.rels[ri].points.len:
      g.rels[ri].points[k] = (g.rels[ri].points[k][0] + o[0], g.rels[ri].points[k][1] + o[1])
  g.avoidObstacles()
  let regionStart = ctx.regions.len
  let linkStart = ctx.links.len
  var svg = g.paintAll(ctx)
  # un libellé d'arête placé à côté de son trait peut sortir à gauche ou en haut :
  # tout décaler plutôt que de le laisser coupé par le cadre de l'image
  var minX, minY = Inf
  for i in regionStart ..< ctx.regions.len:
    minX = min(minX, ctx.regions[i].x)
    minY = min(minY, ctx.regions[i].y)
  let shiftX = if minX < margin / 2: margin / 2 - minX else: 0.0
  let shiftY = if minY < margin / 2: margin / 2 - minY else: 0.0
  if shiftX > 0 or shiftY > 0:
    for e in g.entities:
      e.x += shiftX
      e.y += shiftY
    for ri in 0 ..< g.rels.len:
      for k in 0 ..< g.rels[ri].points.len:
        g.rels[ri].points[k] = (g.rels[ri].points[k][0] + shiftX, g.rels[ri].points[k][1] + shiftY)
    ctx.regions.setLen(regionStart)
    ctx.links.setLen(linkStart)
    svg = g.paintAll(ctx)
  var maxX = w + 2 * margin
  var maxY = h + 2 * margin
  for e in g.entities:
    maxX = max(maxX, e.x + e.w + margin + (if e.shape == shClass: 0.0 else: 0.0))
    maxY = max(maxY, e.y + e.h + margin)
  for r in g.rels:
    if r.a == r.b: maxX = max(maxX, g.entities[r.a].x + g.entities[r.a].w + 60 + blockWidth(r.label.split('\n'), 12))
  # les libellés d'arêtes peuvent déborder de la mise en page
  for i in regionStart ..< ctx.regions.len:
    let rg = ctx.regions[i]
    maxX = max(maxX, rg.x + rg.w + margin / 2)
    maxY = max(maxY, rg.y + rg.h + margin / 2)
  (svg, maxX, maxY)
