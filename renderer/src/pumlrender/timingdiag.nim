## Chronogrammes : robust, concise, clock et binary, changements d'état, messages,
## contraintes, zones surlignées et notes.
import std/[strutils, tables, algorithm, math]
import common, preprocess

type
  PlayerKind = enum pkRobust, pkConcise, pkClock, pkBinary
  Change = object
    t: float
    state, color: string
    line: int
  Player = object
    id, label: string
    kind: PlayerKind
    line: int
    changes: seq[Change]
    period, pulse, offset: float
    y, h: float
  Link = object
    a, b: int    # -1 : axe du temps
    ta, tb: float
    text: string
    line: int
    constraint: bool
  Highlight = object
    a, b: float
    color, text: string
    line: int
  Note = object
    player: int
    t: float
    top: bool
    text: string
    line: int

const MaxClockEdges = 2000

proc renderTiming*(doc: Doc, ctx: Context): (Svg, float, float) =
  var svg: Svg
  var players: seq[Player]
  var byId = initTable[string, int]()
  var anchors = initTable[string, float]()
  var links: seq[Link]
  var marks: seq[Highlight]
  var notes: seq[Note]
  var now = 0.0
  var context = -1  # joueur désigné par « @id »
  var hideAxis = false
  var fixedScale = 0.0

  proc parseTime(s: string, line: int): float =
    var t = s.strip()
    if t.startsWith("+"):
      try: return now + parseFloat(t[1 .. ^1]) except ValueError: discard
    if t.startsWith(":"):
      var name = t[1 .. ^1]
      var delta = 0.0
      for sign in ['+', '-']:
        let p = name.find(sign)
        if p > 0:
          try: delta = parseFloat(name[p .. ^1]) except ValueError: discard
          name = name[0 ..< p]
          break
      if anchors.hasKey(name): return anchors[name] + delta
      ctx.warn(line, "repère de temps inconnu : :" & name)
      return now
    try: return parseFloat(t)
    except ValueError:
      ctx.warn(line, "temps non compris : " & t)
      return now

  proc timeRef(s: string, line: int): (int, float) =
    ## « A@5 », « @5 », « A » → (joueur, temps)
    let at = s.find('@')
    let name = (if at >= 0: s[0 ..< at] else: s).strip()
    let t = if at >= 0: parseTime(s[at + 1 .. ^1], line) else: now
    if name.len == 0: return (-1, t)
    if not byId.hasKey(name):
      ctx.warn(line, "joueur inconnu : " & name)
      return (-1, t)
    (byId[name], t)

  for l in doc.lines:
    let t = l.text.strip()
    if t.len == 0: continue
    let lw = t.toLowerAscii()
    block parsed:
      for (kw, kind) in [("robust", pkRobust), ("concise", pkConcise), ("clock", pkClock), ("binary", pkBinary),
                         ("analog", pkRobust), ("rectangle", pkConcise)]:
        if lw.startsWithWord(kw):
          var rest = t.afterWord(kw)
          var p = Player(kind: kind, line: l.num, period: 1)
          let withAt = rest.toLowerAscii().find(" with ")
          if withAt >= 0:
            let opts = rest[withAt + 6 .. ^1].toLowerAscii().splitWhitespace()
            for k in 0 ..< opts.len - 1:
              try:
                case opts[k]
                of "period": p.period = max(parseFloat(opts[k + 1]), 1e-6)
                of "pulse": p.pulse = parseFloat(opts[k + 1])
                of "offset": p.offset = parseFloat(opts[k + 1])
                else: discard
              except ValueError: ctx.warn(l.num, "valeur d'horloge non comprise : " & opts[k + 1])
            rest = rest[0 ..< withAt].strip()
          let asAt = rest.toLowerAscii().find(" as ")
          if asAt >= 0:
            p.label = stripQuotes(rest[0 ..< asAt])
            p.id = rest[asAt + 4 .. ^1].strip()
          else:
            p.label = stripQuotes(rest)
            p.id = p.label
          if p.id.len == 0:
            ctx.warn(l.num, kw & " sans nom")
            break parsed
          byId[p.id] = players.len
          players.add p
          break parsed
      if t.startsWith("@"):
        var head = t[1 .. ^1]
        let asAt = head.toLowerAscii().find(" as ")
        var anchor = ""
        if asAt >= 0:
          anchor = head[asAt + 4 .. ^1].strip().strip(chars = {':'})
          head = head[0 ..< asAt]
        head = head.strip()
        if byId.hasKey(head):
          context = byId[head]
        else:
          context = -1
          now = parseTime(head, l.num)
          if anchor.len > 0: anchors[anchor] = now
        break parsed
      if lw.startsWithWord("highlight"):
        var rest = t.afterWord("highlight")
        var text = ""
        let colon = rest.find(':')
        if colon >= 0 and not rest[colon .. ^1].startsWith(":") or colon > 0:
          text = rest[colon + 1 .. ^1].strip()
          rest = rest[0 ..< colon].strip()
        var color = ""
        (rest, color) = stripColor(rest.split(';')[0])
        let parts = rest.toLowerAscii().split(" to ")
        if parts.len == 2:
          marks.add Highlight(a: parseTime(parts[0], l.num), b: parseTime(parts[1], l.num),
            color: (if color.len > 0: color else: "#FFF2B3"), text: unescapeLabel(text), line: l.num)
        else:
          ctx.warn(l.num, "highlight non compris : " & t)
        break parsed
      if lw.startsWithWord("note"):
        let colon = t.find(':')
        let head = (if colon >= 0: t[0 ..< colon] else: t).toLowerAscii().splitWhitespace()
        if colon >= 0 and head.len >= 4 and head[2] == "of" and byId.hasKey(t.splitWhitespace()[3]):
          notes.add Note(player: byId[t.splitWhitespace()[3]], t: now, top: head[1] == "top",
            text: unescapeLabel(t[colon + 1 .. ^1].strip()), line: l.num)
        else:
          ctx.warn(l.num, "note de chronogramme non comprise : " & t)
        break parsed
      if lw == "hide time-axis" or lw == "hide time axis":
        hideAxis = true
        break parsed
      if lw.startsWithWord("scale"):
        let parts = lw.splitWhitespace()
        if parts.len >= 5 and parts[2] == "as":
          try: fixedScale = clamp(parseFloat(parts[3]) / max(parseFloat(parts[1]), 1e-6), 0.01, 1000.0)
          except ValueError: discard
        break parsed
      if lw in ["compact", "mode compact", "manual time-axis"] or lw.startsWithWord("use"):
        ctx.info(l.num, "directive de chronogramme ignorée : " & t)
        break parsed
      # messages et contraintes
      for arrow in ["<->", "->", "-->"]:
        let p = t.find(arrow)
        if p > 0 or (p == 0 and arrow == "<->"):
          var after = t[p + arrow.len .. ^1]
          var text = ""
          let colon = after.find(" :")
          if colon >= 0:
            text = after[colon + 2 .. ^1].strip()
            after = after[0 ..< colon]
          elif after.find(':') > 0 and not after.strip().startsWith(":"):
            let c = after.find(':')
            text = after[c + 1 .. ^1].strip()
            after = after[0 ..< c]
          let (a, ta) = timeRef(t[0 ..< p], l.num)
          let (b, tb) = timeRef(after, l.num)
          links.add Link(a: a, b: b, ta: ta, tb: tb, text: unescapeLabel(text), line: l.num, constraint: arrow == "<->")
          break parsed
      # changements d'état
      let isAt = lw.find(" is ")
      if isAt > 0:
        let who = t[0 ..< isAt].strip()
        var value = t[isAt + 4 .. ^1].strip()
        var color = ""
        (value, color) = stripColor(value)
        var player = context
        var time = now
        if context >= 0 and not byId.hasKey(who):
          time = parseTime(who, l.num)
        elif byId.hasKey(who):
          player = byId[who]
        else:
          ctx.warn(l.num, "joueur inconnu : " & who)
          break parsed
        if player >= 0:
          players[player].changes.add Change(t: time, state: stripQuotes(value), color: color, line: l.num)
          break parsed
      ctx.warn(l.num, "syntaxe de chronogramme non reconnue, ligne ignorée : " & t)
  if players.len == 0: return (svg, 0.0, 0.0)

  # étendue du temps
  var tMin = Inf
  var tMax = NegInf
  for p in players.mitems:
    p.changes.sort(proc(a, b: Change): int = cmp(a.t, b.t))
    for c in p.changes:
      tMin = min(tMin, c.t)
      tMax = max(tMax, c.t)
  for k in links:
    tMin = min(tMin, min(k.ta, k.tb))
    tMax = max(tMax, max(k.ta, k.tb))
  for m in marks:
    tMin = min(tMin, min(m.a, m.b))
    tMax = max(tMax, max(m.a, m.b))
  if tMin == Inf: (tMin = 0; tMax = 10)
  if tMax <= tMin: tMax = tMin + 10
  tMax += (tMax - tMin) * 0.1
  let span = tMax - tMin
  var scale = if fixedScale > 0: fixedScale else: 640 / span
  scale = min(scale, 200_000 / span)
  var labelW = 40.0
  var stateW = 0.0
  for p in players:
    labelW = max(labelW, textWidth(p.label, 13, true) + 20)
    if p.kind == pkRobust:
      for c in p.changes: stateW = max(stateW, textWidth(c.state, 10) + 8)
  let x0 = labelW + stateW + 10
  proc tx(t: float): float = x0 + (t - tMin) * scale

  # lignes : une ligne d'état par valeur en mode robust
  var y = 14.0
  for p in players.mitems:
    if p.kind == pkRobust:
      var states: seq[string]
      for c in p.changes:
        if c.state notin states and c.state notin ["{-}", "{hidden}"]: states.add c.state
      p.h = max(states.len, 2).float * 22 + 12
    else:
      p.h = 40
    p.y = y
    y += p.h + 18
  let bottom = y
  let right = tx(tMax) + 20

  # zones surlignées derrière tout le reste
  for m in marks:
    svg.rect(tx(min(m.a, m.b)), 6, abs(m.b - m.a) * scale, bottom - 6, m.color, "none", 0, 0, "", "fill-opacity=\"0.5\"")
    if m.text.len > 0: svg.text(tx(min(m.a, m.b)) + 4, 18, m.text, size = 11, fill = Muted)
    ctx.region(m.line, tx(min(m.a, m.b)), 6, abs(m.b - m.a) * scale, 16)

  for p in players:
    let top = p.y
    let mid = top + p.h / 2
    svg.text(10, mid + 4, p.label, size = 13, bold = true)
    svg.line(x0, top + p.h, right - 10, top + p.h, HeaderFill, 1)
    ctx.region(p.line, 6, top, labelW, p.h)
    case p.kind
    of pkRobust:
      var states: seq[string]
      for c in p.changes:
        if c.state notin states and c.state notin ["{-}", "{hidden}"]: states.add c.state
      proc level(s: string): float =
        let i = states.find(s)
        top + 12 + (states.len - 1 - max(i, 0)).float * 22
      for s in states:
        svg.text(x0 - 6, level(s) + 4, s, "end", 10, fill = Muted)
        svg.line(x0, level(s), right - 10, level(s), HeaderFill, 0.6, "2 3")
      for i, c in p.changes:
        let x1 = tx(c.t)
        let x2 = if i < p.changes.high: tx(p.changes[i + 1].t) else: right - 10
        if c.state in ["{-}", "{hidden}"]: continue
        let ly = level(c.state)
        svg.line(x1, ly, x2, ly, (if c.color.len > 0: c.color else: Stroke), 1.6)
        if i < p.changes.high and p.changes[i + 1].state notin ["{-}", "{hidden}"]:
          svg.line(x2, ly, x2, level(p.changes[i + 1].state), Stroke, 1.6)
        ctx.region(c.line, x1, ly - 6, max(x2 - x1, 6), 12)
    of pkConcise:
      for i, c in p.changes:
        let x1 = tx(c.t)
        let x2 = if i < p.changes.high: tx(p.changes[i + 1].t) else: right - 10
        if c.state in ["{-}", "{hidden}"]:
          svg.line(x1, mid, x2, mid, Muted, 1, "3 3")
          continue
        let notch = min(5.0, (x2 - x1) / 3)
        svg.polygon([(x1, mid), (x1 + notch, top + 8), (x2 - notch, top + 8), (x2, mid), (x2 - notch, top + p.h - 8),
          (x1 + notch, top + p.h - 8)], (if c.color.len > 0: c.color else: Fill))
        svg.text((x1 + x2) / 2, mid + 4, c.state, "middle", 11)
        ctx.region(c.line, x1, top + 8, max(x2 - x1, 6), p.h - 16)
    of pkClock:
      let high = top + 8
      let low = top + p.h - 8
      let width = if p.pulse > 0: p.pulse else: p.period / 2
      var d = "M" & fmtF(tx(tMin)) & " " & fmtF(low)
      var edge = tMin + p.offset
      var count = 0
      while edge < tMax and count < MaxClockEdges:
        d.add " H" & fmtF(tx(edge)) & " V" & fmtF(high) & " H" & fmtF(tx(min(edge + width, tMax))) & " V" & fmtF(low)
        edge += p.period
        inc count
      if count == MaxClockEdges: ctx.warn(p.line, "horloge : période trop courte, tracé limité")
      d.add " H" & fmtF(tx(tMax))
      svg.path(d, "none", Stroke, 1.4)
    of pkBinary:
      let high = top + 8
      let low = top + p.h - 8
      proc level(s: string): float =
        if s.toLowerAscii() in ["high", "1", "true", "on"]: high else: low
      for i, c in p.changes:
        let x1 = tx(c.t)
        let x2 = if i < p.changes.high: tx(p.changes[i + 1].t) else: right - 10
        svg.line(x1, level(c.state), x2, level(c.state), Stroke, 1.6)
        if i < p.changes.high: svg.line(x2, level(c.state), x2, level(p.changes[i + 1].state), Stroke, 1.6)
        ctx.region(c.line, x1, top + 4, max(x2 - x1, 6), p.h - 8)

  # liens
  for k in links:
    let ya = if k.a >= 0: players[k.a].y + players[k.a].h / 2 else: bottom - 8
    let yb = if k.b >= 0: players[k.b].y + players[k.b].h / 2 else: bottom - 8
    let (xa, xb) = (tx(k.ta), tx(k.tb))
    let angle = arctan2(yb - ya, xb - xa)
    svg.line(xa, ya, xb, yb, arrowStroke(), 1.1)
    svg.arrowHead(xb, yb, angle, "filled", arrowStroke())
    if k.constraint: svg.arrowHead(xa, ya, angle + PI, "filled", arrowStroke())
    if k.text.len > 0:
      svg.text((xa + xb) / 2 + 4, (ya + yb) / 2 - 4, k.text, size = 11)
      ctx.region(k.line, (xa + xb) / 2, (ya + yb) / 2 - 14, textWidth(k.text, 11) + 8, 14)

  # notes
  for n in notes:
    let lines = n.text.split('\n')
    let (w, h) = (blockWidth(lines, 11) + 18, blockHeight(lines.len, 11) + 8)
    let p = players[n.player]
    let ny = if n.top: p.y - h + 4 else: p.y + p.h - 4
    svg.note(tx(n.t), ny, w, h)
    svg.textBlock(tx(n.t) + 6, ny + 4, lines, size = 11)
    ctx.region(n.line, tx(n.t), ny, w, h)

  # axe du temps
  var totalH = bottom
  if not hideAxis:
    let step = pow(10.0, floor(log10(span / 8)))
    let nice = if span / step > 40: step * 5 elif span / step > 16: step * 2 else: step
    svg.line(x0, bottom, right - 10, bottom, Stroke, 1)
    var tick = ceil(tMin / nice) * nice
    while tick <= tMax:
      let x = tx(tick)
      svg.line(x, bottom, x, bottom + 5, Stroke, 1)
      svg.text(x, bottom + 17, (if tick == floor(tick): $int(tick) else: fmtF(tick)), "middle", 10, fill = Muted)
      tick += nice
    totalH = bottom + 24
  (svg, right, totalH + 6)
