## Diagrammes de séquence : participants, messages, activations, notes,
## fragments combinés, séparateurs, délais, références et boîtes.
import std/[strutils, tables, math]
import common, preprocess

type
  PKind = enum pkParticipant, pkActor, pkBoundary, pkControl, pkEntity, pkDatabase,
    pkCollections, pkQueue
  Participant = object
    id, label, stereo, color: string
    kind: PKind
    line: int
    x, w, headH: float
    createdAt: float     # >= 0 si créé en cours de diagramme
    destroyedAt: float
    created: bool
    box: int

  EvKind = enum evMsg, evNote, evDivider, evDelay, evSpace, evGroupStart, evGroupElse,
    evGroupEnd, evActivate, evDeactivate, evDestroy, evRef, evCreate
  Event = object
    kind: EvKind
    line: int
    a, b: int            # -1 : bord gauche, -2 : bord droit
    text: string
    dashed: bool
    head, tail: string   # styles de pointe : filled, open, half, cross, circle, ""
    color: string
    activateTarget, deactivateSource, createTarget, destroyTarget: bool
    noteSide: string     # left, right, over
    noteShape: string    # note, hnote, rnote
    span: seq[int]
    size: float
    parallel: bool       # « & » : même hauteur que le message précédent

  Box = object
    title, color: string
    line: int

const PKeywords = [("participant", pkParticipant), ("actor", pkActor), ("boundary", pkBoundary),
  ("control", pkControl), ("entity", pkEntity), ("database", pkDatabase),
  ("collections", pkCollections), ("queue", pkQueue)]

proc renderSequence*(doc: Doc, ctx: Context): (Svg, float, float) =
  var parts: seq[Participant]
  var byId = initTable[string, int]()
  var events: seq[Event]
  var boxes: seq[Box]
  var curBox = -1
  var hideFootbox = false
  var autonum = false
  var autonumVal = 1
  var autonumStep = 1
  var autoActivate = false

  proc getP(id: string, line: int, label = "", kind = pkParticipant): int =
    if id in byId: return byId[id]
    parts.add Participant(id: id, label: (if label.len > 0: label else: id), kind: kind, line: line,
      createdAt: -1, destroyedAt: -1, box: curBox)
    byId[id] = parts.high
    parts.high

  proc readPart(sc: var Scanner): string =
    sc.skipWs()
    if sc.peek == '"': return sc.readQuoted()
    sc.readName(IdentChars + {'.', '$', '\x80'..'\xFF'})

  let lines = doc.lines
  var i = 0
  var parallelFrom = -1  # premier évènement produit par une ligne « & … »
  while i < lines.len:
    if parallelFrom >= 0:
      if parallelFrom < events.len and events[parallelFrom].kind == evMsg: events[parallelFrom].parallel = true
      parallelFrom = -1
    let l = lines[i]
    var t = l.text.strip()
    inc i
    if t.len == 0: continue
    if t.startsWith("&"):
      t = t[1 .. ^1].strip()
      parallelFrom = events.len
    let lw = t.toLowerAscii()
    # --- déclarations
    var declared = false
    var isCreate = false
    var decl = t
    if lw.startsWithWord("create"):
      isCreate = true
      decl = t.afterWord("create")
    for (kw, kind) in PKeywords:
      if decl.toLowerAscii().startsWithWord(kw):
        var sc = initScanner(decl.afterWord(kw))
        var label = ""
        var id = ""
        if sc.peek == '"':
          label = sc.readQuoted()
          id = label
        else:
          id = sc.readName(IdentChars + {'.', '$', '\x80'..'\xFF'})
          label = id
        var stereo, color = ""
        while not sc.eof:
          sc.skipWs()
          if sc.eof: break
          if sc.acceptWord("as"):
            sc.skipWs()
            if sc.peek == '"':
              label = sc.readQuoted()
            else:
              id = sc.readName(IdentChars + {'.', '$', '\x80'..'\xFF'})
          elif sc.accept("<<"):
            let e = sc.s.find(">>", sc.pos)
            if e < 0: break
            stereo = sc.s[sc.pos ..< e].strip()
            sc.pos = e + 2
          elif sc.peek == '#':
            inc sc.pos
            let st = sc.pos
            while not sc.eof and sc.peek notin {' ', '\t'}: inc sc.pos
            color = colorValue(sc.s[st ..< sc.pos])
          else:
            while not sc.eof and sc.peek notin {' ', '\t'}: inc sc.pos
        if id.len == 0:
          ctx.warn(l.num, "participant sans nom")
          declared = true
          break
        let idx = getP(id, l.num, unescapeLabel(label), kind)
        parts[idx].kind = kind
        parts[idx].label = unescapeLabel(label)
        parts[idx].line = l.num
        if stereo.len > 0: parts[idx].stereo = stereo
        if color.len > 0: parts[idx].color = color
        if isCreate:
          parts[idx].created = true
          events.add Event(kind: evCreate, line: l.num, a: idx)
        declared = true
        break
    if declared: continue
    if isCreate:
      var sc = initScanner(decl)
      let id = readPart(sc)
      if id.len > 0:
        let idx = getP(id, l.num)
        parts[idx].created = true
        events.add Event(kind: evCreate, line: l.num, a: idx)
        continue
    # --- directives
    if lw.startsWithWord("autonumber"):
      let rest = t.afterWord("autonumber").toLowerAscii()
      if rest.startsWith("stop"): autonum = false
      elif rest.startsWith("resume"): autonum = true
      else:
        autonum = true
        let toks = rest.splitWhitespace()
        if toks.len > 0:
          try: autonumVal = clamp(parseInt(toks[0]), -1_000_000_000, 1_000_000_000) except ValueError: discard
        if toks.len > 1:
          try: autonumStep = clamp(parseInt(toks[1]), -1_000_000, 1_000_000) except ValueError: discard
      continue
    if lw.startsWith("autoactivate"):
      autoActivate = lw.contains("on")
      continue
    if lw.startsWithWord("hide") or lw.startsWithWord("show"):
      if lw.contains("footbox"): hideFootbox = lw.startsWithWord("hide")
      else: ctx.info(l.num, "directive ignorée : " & t)
      continue
    if lw == "newpage" or lw.startsWith("newpage "):
      ctx.info(l.num, "newpage : toutes les pages sont rendues sur une seule image")
      continue
    if lw.startsWithWord("box"):
      var rest = t.afterWord("box")
      var color = ""
      (rest, color) = stripColor(rest)
      boxes.add Box(title: unescapeLabel(stripQuotes(rest)), color: color, line: l.num)
      curBox = boxes.high
      continue
    if lw == "end box" or lw == "endbox":
      curBox = -1
      continue
    if lw.startsWithWord("activate") or lw.startsWithWord("deactivate") or lw.startsWithWord("destroy"):
      let w = t.split({' ', '\t'})[0].toLowerAscii()
      var rest = t.afterWord(w)
      var color = ""
      (rest, color) = stripColor(rest)
      let id = stripQuotes(rest)
      if id.len == 0: (ctx.warn(l.num, w & " sans participant"); continue)
      let idx = getP(id, l.num)
      let k = case w
        of "activate": evActivate
        of "deactivate": evDeactivate
        else: evDestroy
      events.add Event(kind: k, line: l.num, a: idx, color: color)
      continue
    if lw.startsWith("==") and lw.endsWith("=="):
      events.add Event(kind: evDivider, line: l.num, text: t.strip(chars = {'='}).strip())
      continue
    if lw.startsWith("..."):
      events.add Event(kind: evDelay, line: l.num, text: t.strip(chars = {'.'}).strip())
      continue
    if lw.startsWith("||"):
      var sz = 20.0
      let inner = t.strip(chars = {'|'})
      if inner.len > 0:
        try: sz = parseFloat(inner) except ValueError: discard
      events.add Event(kind: evSpace, line: l.num, size: sz)
      continue
    block groups:
      for kw in ["alt", "opt", "loop", "par", "par2", "break", "critical", "group", "else", "also"]:
        if lw.startsWithWord(kw):
          var label = t.afterWord(kw)
          var color = ""
          if label.startsWith("#"):
            let sp = label.find(' ')
            color = colorValue(if sp > 0: label[0 ..< sp] else: label)
            label = if sp > 0: label[sp .. ^1].strip() else: ""
          if kw in ["else", "also"]:
            events.add Event(kind: evGroupElse, line: l.num, text: unescapeLabel(label))
          else:
            events.add Event(kind: evGroupStart, line: l.num, text: unescapeLabel(label),
              noteShape: (if kw == "par2": "par" else: kw), color: color)
          break groups
      if lw == "end" or lw.startsWith("end ") and not lw.startsWith("end note") and not lw.startsWith("end ref"):
        events.add Event(kind: evGroupEnd, line: l.num)
        continue
      # --- notes et références
      if lw.startsWithWord("note") or lw.startsWithWord("hnote") or lw.startsWithWord("rnote") or lw.startsWithWord("ref"):
        let w = t.split({' ', '\t'})[0].toLowerAscii()
        var rest = t.afterWord(w)
        var color = ""
        (rest, color) = stripColor(rest)
        let colon = rest.find(':')
        let head = (if colon >= 0: rest[0 ..< colon] else: rest).strip()
        var text = if colon >= 0: rest[colon + 1 .. ^1].strip() else: ""
        var side = "right"
        var span: seq[int]
        let hl = head.toLowerAscii()
        var targets = ""
        if hl.startsWithWord("over"):
          side = "over"
          targets = head.afterWord("over")
        elif hl.startsWithWord("left"):
          side = "left"
          targets = head.afterWord("left")
        elif hl.startsWithWord("right"):
          side = "right"
          targets = head.afterWord("right")
        elif hl.startsWithWord("across"):
          side = "across"
        if targets.toLowerAscii().startsWithWord("of"): targets = targets.afterWord("of")
        for part in targets.split(','):
          let p = stripQuotes(part)
          if p.len > 0: span.add getP(p, l.num)
        if span.len == 0 and side != "across":
          # note attachée au dernier message
          for k in countdown(events.high, 0):
            if events[k].kind == evMsg:
              let a = events[k].a
              let b = events[k].b
              # b < 0 marque un retour ou un message vers l'extérieur
              if side == "left": span = @[(if a >= 0: a elif b >= 0: b else: 0)]
              else: span = @[(if b >= 0: b elif a >= 0: a else: 0)]
              break
          if span.len == 0 and parts.len > 0: span = @[0]
          if span.len == 0: span = @[getP("Participant", l.num)]
        if colon < 0:
          var collected: seq[string]
          let endTags = if w == "ref": @["end ref", "endref"] else: @["end note", "endnote", "end hnote", "endhnote", "end rnote", "endrnote"]
          var closed = false
          while i < lines.len:
            let s = lines[i].text
            inc i
            if s.strip().toLowerAscii() in endTags:
              closed = true
              break
            collected.add s.strip()
          if not closed: ctx.error(l.num, w & " sans fin (« " & endTags[0] & " »)")
          text = collected.join("\n")
        events.add Event(kind: (if w == "ref": evRef else: evNote), line: l.num, text: text,
          noteSide: side, noteShape: w, span: span, color: color)
        continue
      # --- retour
      if lw.startsWithWord("return"):
        events.add Event(kind: evMsg, line: l.num, a: -10, b: -10, text: t.afterWord("return"),
          dashed: true, head: "open")
        continue
      # --- messages
      block message:
        var sc = initScanner(t)
        var a = -3
        sc.skipWs()
        if sc.accept("["): a = -1
        elif sc.accept("?"): a = -1
        else:
          let id = readPart(sc)
          if id.len > 0:
            a = -4  # résolu plus tard (après validation de la flèche)
            sc.skipWs()
            let arrowStart = sc.pos
            discard arrowStart
            # stocké temporairement
            var arrow = ""
            while not sc.eof:
              let c = sc.peek
              if c in {'-', '<', '>', '/', '\\', '*'}: (arrow.add c; inc sc.pos)
              elif c in {'x', 'o'} and (arrow.len == 0 or arrow[^1] in {'-', '<', '>'}) and
                  (sc.peek(1) in {'-', '<', '>', ' ', '\t', '[', ']', '?'} or sc.pos + 1 >= sc.s.len):
                if arrow.len == 0 and sc.peek(1) notin {'-', '<'}: break
                arrow.add c
                inc sc.pos
              elif c == '[' and sc.peek(1) == '#':
                let opt = sc.readDelimited('[', ']')
                arrow.add "[" & opt & "]"
              else: break
            if '-' notin arrow: break message
            var b = -3
            sc.skipWs()
            var bid = ""
            if sc.accept("]") or sc.accept("?"): b = -2
            else:
              bid = readPart(sc)
              if bid.len == 0: break message
            sc.skipWs()
            var actT, deactS, createT, destroyT = false
            while sc.peek in {'+', '-', '*', '!'}:
              if sc.accept("++"): actT = true
              elif sc.accept("--"): deactS = true
              elif sc.accept("**"): createT = true
              elif sc.accept("!!"): destroyT = true
              else: inc sc.pos
              sc.skipWs()
            var text = ""
            if sc.accept(":"): text = sc.rest.strip()
            elif not sc.eof: break message
            let ia = getP(id, l.num)
            var ib = if b == -2: -2 else: getP(bid, l.num)
            if createT and ib >= 0:
              parts[ib].created = true
            var ev = Event(kind: evMsg, line: l.num, a: ia, b: ib, text: text, dashed: arrow.contains("--"),
              activateTarget: actT or autoActivate, deactivateSource: deactS, createTarget: createT,
              destroyTarget: destroyT)
            let lb = arrow.find('[')
            if lb >= 0:
              let rb = arrow.find(']', lb)
              ev.color = colorValue(arrow[lb + 1 ..< rb])
              arrow = arrow[0 ..< lb] & arrow[rb + 1 .. ^1]
            let leftHead = arrow.startsWith("<") or arrow.startsWith("x<") or arrow.startsWith("o<") or
              arrow.startsWith("/") or arrow.startsWith("\\")
            let rightHead = arrow.endsWith(">") or arrow.endsWith(">x") or arrow.endsWith(">o") or
              arrow.endsWith("\\") or arrow.endsWith("/") or arrow.endsWith("x")
            proc style(s: string, isRight: bool): string =
              if s.contains(">>") or s.contains("<<"): "open"
              elif (isRight and (s.endsWith("\\") or s.endsWith("/"))) or (not isRight and (s.startsWith("/") or s.startsWith("\\"))): "half"
              else: "filled"
            if leftHead and not rightHead:
              swap(ev.a, ev.b)
              if ev.a == -2: ev.a = -1
              ev.head = style(arrow, false)
              if arrow.startsWith("x"): ev.head = "cross"
              if arrow.endsWith("o"): ev.tail = "circle"
            else:
              ev.head = style(arrow, true)
              if arrow.endsWith("x"): ev.head = "cross"
              if arrow.endsWith("o"): ev.tail = "circle2"
              if arrow.startsWith("o"): ev.tail = "circle"
              if leftHead: ev.tail = style(arrow, false)
            if autonum:
              ev.text = $autonumVal & "  " & ev.text
              autonumVal += autonumStep
            events.add ev
            continue
        if a == -1:
          # message entrant depuis la bordure
          var arrow = ""
          while not sc.eof and sc.peek in {'-', '>', '<', '/', '\\', 'x', 'o'}:
            arrow.add sc.peek
            inc sc.pos
          if '-' in arrow:
            let bid = readPart(sc)
            if bid.len > 0:
              sc.skipWs()
              var text = ""
              if sc.accept(":"): text = sc.rest.strip()
              let ib = getP(bid, l.num)
              var ev = Event(kind: evMsg, line: l.num, a: -1, b: ib, text: text, dashed: arrow.contains("--"),
                head: (if arrow.contains(">>"): "open" else: "filled"))
              if arrow.startsWith("<"): (ev.a = ib; ev.b = -1)
              if autonum:
                ev.text = $autonumVal & "  " & ev.text
                autonumVal += autonumStep
              events.add ev
              continue
      ctx.warn(l.num, "syntaxe de séquence non reconnue, ligne ignorée : " & t)

  if parallelFrom >= 0 and parallelFrom < events.len and events[parallelFrom].kind == evMsg:
    events[parallelFrom].parallel = true
  if parts.len == 0:
    var svg: Svg
    return (svg, 0.0, 0.0)

  # ---------------------------------------------------------- dimensions
  for p in parts.mitems:
    let lines = splitLabel(p.label)
    let tw = blockWidth(lines) + (if p.stereo.len > 0: 0.0 else: 0.0)
    let sw = if p.stereo.len > 0: textWidth("«" & p.stereo & "»", 11) else: 0.0
    let lh = blockHeight(lines.len) + (if p.stereo.len > 0: 14.0 else: 0.0)
    case p.kind
    of pkParticipant, pkCollections, pkQueue:
      p.w = max(max(tw, sw) + 24, 46)
      p.headH = lh + 16
    of pkDatabase:
      p.w = max(max(tw, sw) + 24, 46)
      p.headH = lh + 34
    of pkActor:
      p.w = max(max(tw, sw) + 6, 40)
      p.headH = lh + 50
    else:
      p.w = max(max(tw, sw) + 6, 46)
      p.headH = lh + 36
  var headH = 0.0
  for p in parts: headH = max(headH, p.headH)
  let n = parts.len
  var cx = newSeq[float](n)
  var x = 0.0
  for k in 0 ..< n:
    if k > 0: x += parts[k - 1].w / 2 + 30 + parts[k].w / 2
    cx[k] = x
  var leftMargin = parts[0].w / 2 + 20
  var rightMargin = parts[^1].w / 2 + 20
  proc need(a, b: int, d: float) =
    if a < 0 or b < 0 or a == b: return
    let (lo, hi) = (min(a, b), max(a, b))
    let deficit = d - (cx[hi] - cx[lo])
    if deficit > 0:
      for k in hi ..< n: cx[k] += deficit
  var stackCallers: seq[int]
  for ev in events:
    case ev.kind
    of evMsg:
      let lines = splitLabel(ev.text)
      let tw = blockWidth(lines, 12)
      if ev.a >= 0 and ev.b >= 0 and ev.a != ev.b: need(ev.a, ev.b, tw + 30)
      elif ev.a >= 0 and ev.a == ev.b:
        if ev.a + 1 < n: need(ev.a, ev.a + 1, tw + 60)
        else: rightMargin = max(rightMargin, tw + 60)
      elif ev.a == -1 and ev.b >= 0:
        if ev.b == 0: leftMargin = max(leftMargin, tw + 40)
        else: need(0, ev.b, 0)
      elif ev.b == -2 or ev.b == -1:
        if ev.a == n - 1 or ev.b == -2: rightMargin = max(rightMargin, tw + 40)
    of evNote, evRef:
      let lines = splitLabel(ev.text)
      let tw = blockWidth(lines) + 24
      if ev.noteSide == "over" or ev.kind == evRef:
        if ev.span.len == 1:
          let p = ev.span[0]
          if p == 0: leftMargin = max(leftMargin, tw / 2 + 10)
          if p == n - 1: rightMargin = max(rightMargin, tw / 2 + 10)
          if p > 0: need(p - 1, p, tw / 2 + parts[p - 1].w / 2 + 10)
          if p + 1 < n: need(p, p + 1, tw / 2 + parts[p + 1].w / 2 + 10)
        elif ev.span.len >= 2:
          let lo = min(ev.span)
          let hi = max(ev.span)
          need(lo, hi, max(0.0, tw - 40))
      elif ev.noteSide == "left":
        let p = ev.span[0]
        if p == 0: leftMargin = max(leftMargin, tw + 20)
        else: need(p - 1, p, tw + 30)
      elif ev.noteSide == "right":
        let p = ev.span[0]
        if p == n - 1: rightMargin = max(rightMargin, tw + 20)
        else: need(p, p + 1, tw + 30)
    of evGroupStart:
      leftMargin = max(leftMargin, 30)
    else: discard
  discard stackCallers
  for k in 0 ..< n: cx[k] += leftMargin
  let width = cx[^1] + rightMargin

  # ---------------------------------------------------------- dessin
  var bg, mid, fg: Svg
  var y = headH + 16.0
  let top0 = 6.0
  var active = newSeq[seq[(float, string, int)]](n)   # piles (début, couleur, ligne)
  var bars: seq[(int, float, float, int, string, int)] # participant, y0, y1, niveau, couleur, ligne
  type Grp = object
    kind, label, color: string
    y0: float
    lo, hi: int
    elses: seq[(float, string, int)]
    line: int
    xmin, xmax: float   ## étendue des auto-messages et notes qui dépassent des colonnes
  var groups: seq[Grp]
  var callStack: seq[(int, int)]
  proc touch(cols: varargs[int]) =
    for g in groups.mitems:
      for c in cols:
        if c >= 0:
          g.lo = min(g.lo, c)
          g.hi = max(g.hi, c)
  var rightmost = 0.0
  proc extent(x0, x1: float) =
    for g in groups.mitems:
      g.xmin = min(g.xmin, x0)
      g.xmax = max(g.xmax, x1)
  proc depth(p: int): int = (if p >= 0: active[p].len else: 0)
  proc px(p: int, side: float): float =
    ## abscisse d'une extrémité de message, compte tenu des activations
    if p == -1: return 6.0
    if p == -2: return width - 6
    let d = depth(p)
    if d == 0: cx[p] else: cx[p] + side * 5 + float(d - 1) * 5
  proc drawHead(svg: var Svg, x, y: float, dir: float, style, color: string) =
    case style
    of "filled": svg.arrowHead(x, y, dir, "filled", color)
    of "open": svg.arrowHead(x, y, dir, "open", color)
    of "half": svg.arrowHead(x, y, dir, "half", color)
    of "cross": svg.arrowHead(x, y, dir, "cross", color)
    of "circle", "circle2": svg.arrowHead(x, y, dir, "circle", color)
    else: discard
  proc headerBoxBody(svg: var Svg, p: Participant, xc, top: float) =
    let lines = splitLabel(p.label)
    let fill = if p.color.len > 0: p.color else: Fill
    let st = if p.stereo.len > 0: "«" & p.stereo & "»" else: ""
    let lh = blockHeight(lines.len) + (if st.len > 0: 14.0 else: 0.0)
    proc label(svg: var Svg, ty: float) =
      var yy = ty
      if st.len > 0:
        svg.text(xc, yy + 11, st, "middle", 11, italic = true, fill = Muted)
        yy += 14
      svg.textBlock(xc, yy, lines, "middle")
    case p.kind
    of pkParticipant:
      svg.rect(xc - p.w / 2, top, p.w, p.headH, fill, Stroke, 3)
      label(svg, top + 8)
    of pkCollections:
      svg.rect(xc - p.w / 2 + 4, top - 4, p.w, p.headH, fill, Stroke, 3)
      svg.rect(xc - p.w / 2, top, p.w, p.headH, fill, Stroke, 3)
      label(svg, top + 8)
    of pkQueue:
      svg.rect(xc - p.w / 2, top, p.w, p.headH, fill, Stroke, 10)
      label(svg, top + 8)
    of pkDatabase:
      let w = p.w
      let x0 = xc - w / 2
      let h = p.headH
      svg.path("M" & fmtF(x0) & " " & fmtF(top + 7) & " A" & fmtF(w / 2) & " 7 0 0 1 " & fmtF(x0 + w) & " " &
        fmtF(top + 7) & " V" & fmtF(top + h - 7) & " A" & fmtF(w / 2) & " 7 0 0 1 " & fmtF(x0) & " " &
        fmtF(top + h - 7) & " Z", fill)
      svg.path("M" & fmtF(x0) & " " & fmtF(top + 7) & " A" & fmtF(w / 2) & " 7 0 0 0 " & fmtF(x0 + w) & " " & fmtF(top + 7))
      label(svg, top + 18)
    of pkActor:
      svg.circle(xc, top + 8, 7.5, fill)
      svg.line(xc, top + 15.5, xc, top + 31)
      svg.line(xc - 13, top + 21, xc + 13, top + 21)
      svg.line(xc, top + 31, xc - 11, top + 45)
      svg.line(xc, top + 31, xc + 11, top + 45)
      label(svg, top + 48)
    of pkBoundary:
      svg.line(xc - 18, top + 4, xc - 18, top + 26)
      svg.line(xc - 18, top + 15, xc - 10, top + 15)
      svg.circle(xc + 2, top + 15, 12, fill)
      label(svg, top + 32)
    of pkControl:
      svg.circle(xc, top + 16, 12, fill)
      svg.arrowHead(xc + 3, top + 4, PI, "open")
      label(svg, top + 32)
    of pkEntity:
      svg.circle(xc, top + 14, 12, fill)
      svg.line(xc - 12, top + 28, xc + 12, top + 28)
      label(svg, top + 32)
    discard lh

  var lastMsgTo = newSeq[int](n)
  for k in 0 ..< n: lastMsgTo[k] = -1
  proc headerBox(svg: var Svg, p: Participant, xc, top: float) =
    styled("participant"):
      headerBoxBody(svg, p, xc, top)
  proc lastFrom(p: int): int = lastMsgTo[p]
  var createdY = newSeq[float](n)
  for k in 0 ..< n: createdY[k] = -1
  var destroyedY = newSeq[float](n)
  for k in 0 ..< n: destroyedY[k] = -1
  for k in 0 ..< n:
    if not parts[k].created:
      fg.headerBox(parts[k], cx[k], top0 + headH - parts[k].headH)
      ctx.region(parts[k].line, cx[k] - parts[k].w / 2, top0 + headH - parts[k].headH, parts[k].w, parts[k].headH)

  proc createAt(p: int, yy: float, line: int) =
    if p < 0 or createdY[p] >= 0: return
    createdY[p] = yy - parts[p].headH / 2
    fg.headerBox(parts[p], cx[p], createdY[p])
    ctx.region(parts[p].line, cx[p] - parts[p].w / 2, createdY[p], parts[p].w, parts[p].headH)

  var msgTop, msgBottom = 0.0
  for ev in events:
    # « & A -> B » : le message démarre à la hauteur du précédent ; la suite reprend sous le plus bas
    if ev.kind == evMsg and ev.parallel:
      msgBottom = max(msgBottom, y)
      y = msgTop
    else:
      y = max(y, msgBottom)
      msgBottom = 0
      if ev.kind == evMsg: msgTop = y
    case ev.kind
    of evCreate:
      discard
    of evMsg:
      var a = ev.a
      var b = ev.b
      if a == -10:
        # return : du dernier appelé vers son appelant
        if callStack.len > 0:
          let (caller, callee) = callStack.pop()
          a = callee
          b = caller
          if a >= 0 and active[a].len > 0:
            let (y0, col, barLine) = active[a].pop()
            bars.add (a, y0, y + 8, active[a].len, col, barLine)
        else:
          ctx.warn(ev.line, "return sans appel actif")
          continue
      let lines = splitLabel(ev.text)
      let lh = blockHeight(lines.len, 12)
      let color = if ev.color.len > 0: ev.color else: arrowStroke()
      let dash = if ev.dashed: "6 4" else: ""
      touch(a, b)
      if a >= 0 and a == b:
        let x0 = px(a, 1)
        let ty = y
        fg.textBlock(x0 + 8, ty, lines, size = 12)
        let y0 = ty + lh + 4
        let y1 = y0 + 16
        let tw = blockWidth(lines, 12)
        fg.path("M" & fmtF(x0) & " " & fmtF(y0) & " H" & fmtF(x0 + 36) & " V" & fmtF(y1) & " H" & fmtF(x0 + 2),
          "none", color, 1.2, dash)
        fg.drawHead(x0 + 2, y1, PI, ev.head, color)
        ctx.link(ev.line, [(x0, y0), (x0 + 36, y0), (x0 + 36, y1), (x0 + 2, y1)])
        if tw > 0: ctx.region(ev.line, x0, ty, tw + 10, lh + 4)
        extent(x0, x0 + 8 + max(tw, 36))
        y = y1 + 14
      else:
        if ev.createTarget and b >= 0: createAt(b, y + lh + 6, ev.line)
        if b >= 0 and a >= 0: lastMsgTo[b] = a
        let dir = if a == -1: 1.0 elif b == -1: -1.0 elif b == -2: 1.0 elif a == -2: -1.0
                  elif cx[b] >= cx[a]: 1.0 else: -1.0
        var x0 = px(a, dir)
        var x1 = px(b, -dir)
        if b >= 0 and createdY[b] >= 0 and ev.createTarget:
          x1 = cx[b] - dir * parts[b].w / 2
        let ly = y + lh + 4
        let tx = (x0 + x1) / 2
        fg.textBlock(min(x0, x1) + 8, y, lines, size = 12)
        fg.line(x0, ly, x1, ly, color, 1.2, dash)
        fg.drawHead(x1, ly, (if dir > 0: 0.0 else: PI), ev.head, color)
        if ev.tail.len > 0:
          fg.drawHead(x0, ly, (if dir > 0: PI else: 0.0), ev.tail, color)
        discard tx
        ctx.link(ev.line, [(x0, ly), (x1, ly)])
        # le libellé seul : la flèche se clique sur son tracé
        let labelW = blockWidth(lines, 12)
        if labelW > 0: ctx.region(ev.line, min(x0, x1) + 6, y, labelW + 4, lh + 2)
        y = ly + 14
        if b >= 0 and ev.activateTarget:
          active[b].add (ly, "", ev.line)
          if a >= 0: callStack.add (a, b)
        if a >= 0 and ev.deactivateSource and active[a].len > 0:
          let (y0, col, barLine) = active[a].pop()
          bars.add (a, y0, ly, active[a].len, col, barLine)
        if b >= 0 and ev.destroyTarget:
          destroyedY[b] = ly
          fg.arrowHead(cx[b] + 7, ly + 7, PI / 4, "cross", Ink)
    of evActivate:
      let p = ev.a
      active[p].add (y - 10, ev.color, ev.line)
      touch(p)
      # l'appelant est l'émetteur du dernier message reçu par p
      callStack.add (lastFrom(p), p)
    of evDeactivate:
      let p = ev.a
      if active[p].len > 0:
        let (y0, col, barLine) = active[p].pop()
        bars.add (p, y0, y - 10, active[p].len, col, barLine)
        for k in countdown(callStack.high, 0):
          if callStack[k][1] == p:
            callStack.delete(k)
            break
      else: ctx.warn(ev.line, "deactivate sans activation : " & parts[p].label)
    of evDestroy:
      let p = ev.a
      while active[p].len > 0:
        let (y0, col, barLine) = active[p].pop()
        bars.add (p, y0, y - 6, active[p].len, col, barLine)
      destroyedY[p] = y - 6
      fg.arrowHead(cx[p] + 7, y + 1, PI / 4, "cross", Ink)
      y += 12
    of evNote, evRef:
      let lines = splitLabel(ev.text)
      let tw = blockWidth(lines) + 20
      let th = blockHeight(lines.len) + 12 + (if ev.kind == evRef: 18.0 else: 0.0)
      var x0, w: float
      if ev.noteSide == "across":
        x0 = cx[0] - parts[0].w / 2
        w = cx[^1] + parts[^1].w / 2 - x0
      elif ev.noteSide == "over" or ev.kind == evRef:
        let lo = if ev.span.len > 0: min(ev.span) else: 0
        let hi = if ev.span.len > 0: max(ev.span) else: 0
        if lo == hi:
          w = max(tw, 40)
          x0 = cx[lo] - w / 2
        else:
          x0 = cx[lo] - 20
          w = max(cx[hi] - cx[lo] + 40, tw)
      elif ev.noteSide == "left":
        w = tw
        x0 = cx[ev.span[0]] - 12 - w
      else:
        w = tw
        x0 = cx[ev.span[0]] + 12
      for p in ev.span: touch(p)
      extent(x0, x0 + w)
      let fill = if ev.color.len > 0: ev.color else: NoteFill
      if ev.kind == evRef:
        mid.rect(x0, y, w, th, Background, Stroke, 0, 1.4)
        mid.path("M" & fmtF(x0) & " " & fmtF(y + 18) & " H" & fmtF(x0 + 34) & " L" & fmtF(x0 + 40) & " " &
          fmtF(y + 12) & " V" & fmtF(y), HeaderFill)
        mid.text(x0 + 6, y + 13, "ref", size = 11, bold = true)
        mid.textBlock(x0 + w / 2, y + 22, lines, "middle")
      else:
        case ev.noteShape
        of "hnote":
          fg.polygon([(x0, y + th / 2), (x0 + 8, y), (x0 + w - 8, y), (x0 + w, y + th / 2), (x0 + w - 8, y + th), (x0 + 8, y + th)], fill, NoteStroke)
        of "rnote":
          fg.rect(x0, y, w, th, fill, NoteStroke)
        else:
          fg.note(x0, y, w, th, fill)
        fg.textBlock(x0 + 10, y + 6, lines)
      ctx.region(ev.line, x0, y, w, th)
      y += th + 12
    of evDivider:
      fg.line(0, y + 6, width, y + 6, Muted, 1)
      fg.line(0, y + 10, width, y + 10, Muted, 1)
      if ev.text.len > 0:
        let tw = textWidth(ev.text, 12, true) + 20
        fg.rect(width / 2 - tw / 2, y, tw, 16, HeaderFill, Stroke, 2)
        fg.text(width / 2, y + 12, ev.text, "middle", 12, bold = true)
        ctx.region(ev.line, width / 2 - tw / 2, y, tw, 16)
      y += 28
    of evDelay:
      if ev.text.len > 0:
        fg.text(width / 2, y + 16, ev.text, "middle", 12, italic = true, fill = Muted)
        ctx.region(ev.line, width / 2 - textWidth(ev.text, 12) / 2, y + 4, textWidth(ev.text, 12), 16)
      bg.add "<!--delay:" & fmtF(y) & "-->\n"
      y += 30
    of evSpace:
      y += ev.size
    of evGroupStart:
      if groups.len >= MaxNesting: tooDeep(ev.line)
      groups.add Grp(kind: ev.noteShape, label: ev.text, color: ev.color, y0: y, lo: high(int), hi: -1, line: ev.line,
        xmin: Inf, xmax: NegInf)
      y += 26
    of evGroupElse:
      if groups.len == 0:
        ctx.warn(ev.line, "else hors d'un fragment")
      else:
        groups[^1].elses.add (y, ev.text, ev.line)
        y += 20
    of evGroupEnd:
      if groups.len == 0:
        ctx.warn(ev.line, "end sans fragment ouvert")
        continue
      let g = groups.pop()
      var lo = g.lo
      var hi = g.hi
      if hi < 0: (lo = 0; hi = n - 1)
      let x0 = cx[lo] - parts[lo].w / 2 - 10 + float(groups.len) * 0
      let x1 = cx[hi] + parts[hi].w / 2 + 10
      let pad = float(groups.len) * 6
      let gx0 = min(x0 - 6 + pad, g.xmin - 12 + pad)
      let gx1 = max(x1 + 6 - pad, g.xmax + 12 - pad)
      rightmost = max(rightmost, gx1)
      # le cadre interne compte aussi dans l'étendue du cadre qui le contient
      extent(gx0, gx1)
      mid.rect(gx0, g.y0, gx1 - gx0, y - g.y0 + 2, (if g.color.len > 0: g.color else: "none"), Stroke, 0, 1.3,
        extra = (if g.color.len > 0: "fill-opacity=\"0.25\"" else: ""))
      let kw = g.kind
      let kwW = textWidth(kw, 12, true) + 16
      mid.path("M" & fmtF(gx0) & " " & fmtF(g.y0 + 18) & " H" & fmtF(gx0 + kwW) & " L" & fmtF(gx0 + kwW + 7) & " " &
        fmtF(g.y0 + 11) & " V" & fmtF(g.y0), HeaderFill)
      mid.text(gx0 + 6, g.y0 + 13, kw, size = 12, bold = true)
      if g.label.len > 0:
        mid.text(gx0 + kwW + 14, g.y0 + 13, "[" & g.label & "]", size = 12, bold = true)
      for (ey, et, elseLine) in g.elses:
        mid.line(gx0, ey, gx1, ey, Stroke, 1.1, "5 4")
        ctx.region(elseLine, gx0, ey, max(textWidth("[" & et & "]", 12, true) + 16, 40), 18)
        if et.len > 0: mid.text(gx0 + 8, ey + 13, "[" & et & "]", size = 12, bold = true)
      ctx.region(g.line, gx0, g.y0, kwW + 14 + (if g.label.len > 0: textWidth("[" & g.label & "]", 12, true) else: 0.0), 18)
      touch(lo, hi)
      y += 14
  y = max(y, msgBottom)
  for g in groups:
    ctx.error(g.line, "fragment « " & g.kind & " » non fermé (end manquant)")
  # activations restantes
  for p in 0 ..< n:
    while active[p].len > 0:
      let (y0, col, barLine) = active[p].pop()
      bars.add (p, y0, y, active[p].len, col, barLine)
  let bottom = y + 6
  # lignes de vie
  for k in 0 ..< n:
    let startY = if createdY[k] >= 0: createdY[k] + parts[k].headH else: top0 + headH
    let endY = if destroyedY[k] >= 0: destroyedY[k] else: bottom
    let lifeline = Elements.getOrDefault("sequencelifeline").border
    bg.line(cx[k], startY, cx[k], endY, (if lifeline.len > 0: lifeline else: Muted), 1, "5 5")
  for (p, y0, y1, lvl, col, barLine) in bars:
    bg.rect(cx[p] - 5 + float(lvl) * 5, y0, 10, max(4.0, y1 - y0), (if col.len > 0: col else: Background), Stroke, 0, 1.1)
    ctx.region(barLine, cx[p] - 5 + float(lvl) * 5, y0, 10, max(4.0, y1 - y0))
  # boîtes de participants
  var boxSvg: Svg
  for bi, b in boxes:
    var lo = high(int)
    var hi = -1
    for k in 0 ..< n:
      if parts[k].box == bi:
        lo = min(lo, k)
        hi = max(hi, k)
    if hi < 0: continue
    let x0 = cx[lo] - parts[lo].w / 2 - 8
    let x1 = cx[hi] + parts[hi].w / 2 + 8
    let bh = bottom + (if hideFootbox: 4.0 else: headH + 4) - 0
    boxSvg.rect(x0, 0, x1 - x0, bh + 4, (if b.color.len > 0: b.color else: HeaderFill), Muted, 4, 1)
    if b.title.len > 0:
      boxSvg.text((x0 + x1) / 2, 12, b.title, "middle", 11, bold = true, fill = Muted)
    ctx.region(b.line, x0, 0, x1 - x0, 16)
  var totalH = bottom
  if not hideFootbox:
    for k in 0 ..< n:
      if destroyedY[k] < 0:
        fg.headerBox(parts[k], cx[k], bottom)
    totalH = bottom + headH + 8
  var svg: Svg
  svg.body = boxSvg.body & bg.body & mid.body & fg.body
  (svg, max(width, rightmost + 10), totalH + 6)
