## Diagrammes de Gantt (@startgantt) : tâches, durées, dépendances,
## jalons, avancement et séparateurs. Les jours fermés ne sont pas gérés.
import std/[strutils, tables, times]
import common, preprocess

type
  Task = object
    name, alias: string
    line: int
    start, duration: int
    anchor: int  ## tâche dont le début sert de début (-1 : aucune)
    startKnown: bool
    milestone: bool
    complete: int
    color: string
    deps: seq[int]
    separator: bool

const MaxDays = 3660  ## au-delà (dix ans), l'aperçu deviendrait démesuré

proc renderGantt*(doc: Doc, ctx: Context): (Svg, float, float) =
  var tasks: seq[Task]
  var byName = initTable[string, int]()
  var projectStart: DateTime
  var hasStart = false
  var last = -1
  var links: seq[(int, int)]

  proc bounded(n, line: int, what: string): int =
    ## Borne un nombre de jours : une valeur démesurée, souvent saisie en
    ## cours de frappe, ne doit pas bloquer l'aperçu.
    result = clamp(n, 0, MaxDays)
    if result != n:
      ctx.warn(line, what & " hors de l'intervalle 0 à " & $MaxDays & " jours : valeur bornée")

  proc dateOffset(s: string, line: int): int =
    let t = s.strip()
    try:
      let d = parse(t, "yyyy-MM-dd")
      if not hasStart:
        ctx.warn(line, "date utilisée sans « Project starts » : origine au " & t)
        projectStart = d
        hasStart = true
      return bounded(int((d - projectStart).inDays), line, "début")
    except CatchableError:
      ctx.warn(line, "date non comprise : " & t)
      return 0

  proc task(name: string, line: int): int =
    let key = name.strip()
    if key in byName: return byName[key]
    tasks.add Task(name: key, line: line, duration: 1, anchor: -1)
    byName[key] = tasks.high
    tasks.high

  proc readTask(s: var string): string =
    ## Lit [Nom] en tête de s.
    s = s.strip()
    if not s.startsWith("["): return ""
    let e = s.find(']')
    if e < 0: return ""
    result = s[1 ..< e]
    s = s[e + 1 .. ^1].strip()

  for l in doc.lines:
    var t = l.text.strip()
    if t.len == 0: continue
    let lw = t.toLowerAscii()
    if lw.startsWith("project starts") or lw.startsWith("project start"):
      let toks = t.splitWhitespace()
      let d = toks[^1]
      try:
        projectStart = parse(d, "yyyy-MM-dd")
        hasStart = true
      except CatchableError:
        ctx.warn(l.num, "date de début non comprise (format AAAA-MM-JJ attendu)")
      continue
    if t.startsWith("--") and t.endsWith("--"):
      tasks.add Task(name: t.strip(chars = {'-'}).strip(), line: l.num, separator: true, anchor: -1)
      continue
    if lw.contains(" are closed") or lw.contains(" is closed") or lw.contains(" are open") or
        lw.startsWith("printscale") or lw.startsWith("language") or lw.startsWith("projectscale") or
        lw.contains("zoom"):
      ctx.info(l.num, "directive de calendrier ignorée : " & t)
      continue
    var thenMode = false
    if lw.startsWithWord("then"):
      thenMode = true
      t = t.afterWord("then")
    var rest = t
    let name = readTask(rest)
    if name.len == 0:
      ctx.warn(l.num, "ligne de Gantt non reconnue : " & t)
      continue
    # dépendance [A] -> [B]
    if rest.startsWith("->"):
      var r2 = rest[2 .. ^1]
      let other = readTask(r2)
      if other.len > 0:
        let a = task(name, l.num)
        let b = task(other, l.num)
        tasks[b].deps.add a
        links.add (a, b)
        continue
    var idx = task(name, l.num)
    if rest.toLowerAscii().startsWithWord("as"):
      var r2 = rest.afterWord("as")
      let alias = readTask(r2)
      if alias.len > 0:
        byName[alias] = idx
        tasks[idx].alias = alias
      rest = r2
    if thenMode and last >= 0 and not tasks[idx].startKnown:
      tasks[idx].deps.add last
    for clauseRaw in rest.split(" and "):
      let clause = clauseRaw.strip()
      let cl = clause.toLowerAscii()
      if cl.startsWith("lasts") or cl.startsWith("requires"):
        let toks = clause.splitWhitespace()
        if toks.len >= 2:
          try:
            # borné avant toute multiplication : pas de débordement
            var n = clamp(parseInt(toks[1]), -1, MaxDays + 1)
            if toks.len >= 3 and toks[2].toLowerAscii().startsWith("week"): n *= 7
            tasks[idx].duration = bounded(n, l.num, "durée")
          except ValueError: ctx.warn(l.num, "durée non comprise : " & clause)
      elif cl.startsWith("starts") or cl.startsWith("happens"):
        let isMilestone = cl.startsWith("happens")
        if isMilestone:
          tasks[idx].milestone = true
          tasks[idx].duration = 0
        var r = clause.afterWord(if isMilestone: "happens" else: "starts")
        var rl = r.toLowerAscii()
        for w in ["at", "after", "on"]:
          if rl.startsWithWord(w):
            r = r.afterWord(w)
            rl = r.toLowerAscii()
        if r.startsWith("["):
          var r2 = r
          let other = readTask(r2)
          let o = task(other, l.num)
          if r2.toLowerAscii().contains("start"):
            tasks[idx].anchor = o  # ancrage au début (résolu plus bas)
            tasks[idx].startKnown = true
          else:
            tasks[idx].deps.add o
        elif rl.len > 0 and rl[0] in Digits and rl.contains('-'):
          tasks[idx].start = dateOffset(r, l.num)
          tasks[idx].startKnown = true
        elif rl.startsWith("d+") or rl.startsWith("j+"):
          try:
            tasks[idx].start = bounded(parseInt(r[2 .. ^1].strip()), l.num, "début")
            tasks[idx].startKnown = true
          except ValueError: discard
      elif cl.startsWith("ends") :
        ctx.info(l.num, "contrainte de fin simplifiée : " & clause)
      elif cl.startsWith("is") and cl.contains("% complete"):
        let p = cl.find('%')
        let toks = cl[0 ..< p].splitWhitespace()
        try: tasks[idx].complete = parseInt(toks[^1]) except ValueError: discard
      elif cl.startsWith("is colored in") or cl.startsWith("is colored"):
        let c = clause.splitWhitespace()[^1].split('/')[0]
        tasks[idx].color = colorValue(c)
      elif cl.len > 0:
        ctx.warn(l.num, "clause de Gantt non comprise : " & clause)
    last = idx

  # résolution des débuts (dépendances : fin de la dépendance la plus tardive)
  # parcours itératif : une très longue chaîne ne doit pas épuiser la pile
  var resolved = newSeq[bool](tasks.len)
  var visiting = newSeq[bool](tasks.len)
  for root in 0 ..< tasks.len:
    var stack = @[root]
    while stack.len > 0:
      let i = stack[^1]
      if resolved[i]:
        discard stack.pop()
        continue
      if not visiting[i]:
        visiting[i] = true
        var needed = tasks[i].deps
        if tasks[i].anchor >= 0 and tasks[i].anchor != i: needed.add tasks[i].anchor
        for d in needed:
          if not resolved[d] and not visiting[d]: stack.add d
        if stack[^1] != i: continue
      let o = tasks[i].anchor
      if o >= 0 and o != i: tasks[i].start = tasks[o].start
      for d in tasks[i].deps:
        tasks[i].start = max(tasks[i].start, tasks[d].start + tasks[d].duration)
      # une chaîne de dépendances peut encore dépasser la borne
      tasks[i].start = min(tasks[i].start, MaxDays)
      tasks[i].duration = min(tasks[i].duration, MaxDays)
      resolved[i] = true
      discard stack.pop()

  var svg: Svg
  var real = 0
  var span = 1
  var nameW = 60.0
  for t in tasks:
    if not t.separator:
      inc real
      span = max(span, t.start + max(t.duration, 1))
    nameW = max(nameW, textWidth(t.name, 12) + 20)
  span = min(span, 2 * MaxDays)
  if tasks.len == 0: return (svg, 0.0, 0.0)
  let dayW = max(16.0, min(34.0, 760.0 / float(span)))
  let headH = if hasStart: 34.0 else: 22.0
  let rowH = 24.0
  let left = 10.0
  let gx = left + nameW
  let width = gx + float(span) * dayW + 20
  let height = headH + float(tasks.len) * rowH + 10
  # grille
  for d in 0 ..< span:
    let x = gx + float(d) * dayW
    var label = $(d + 1)
    var weekend = false
    if hasStart:
      let day = projectStart + initDuration(days = d)
      label = $day.monthday
      weekend = day.weekday in {dSat, dSun}
      if day.monthday == 1 or d == 0:
        svg.text(x + 2, 12, day.format("MMM yyyy"), size = 10, bold = true, fill = Muted)
    if weekend: svg.rect(x, headH - 16, dayW, height - headH + 16, "#F1F3F7", "none", 0, 0)
    svg.line(x, headH - 16, x, height - 6, "#E1E5EC", 1)
    svg.text(x + dayW / 2, headH - 4, label, "middle", 10, fill = Muted)
  svg.line(gx, headH, width - 20, headH, "#C7CEDB", 1)
  var rowOf = newSeq[float](tasks.len)
  for i, t in tasks:
    let y = headH + float(i) * rowH
    rowOf[i] = y
    if t.separator:
      svg.line(left, y + rowH / 2, width - 20, y + rowH / 2, Muted, 1)
      if t.name.len > 0:
        let tw = textWidth(t.name, 11, true) + 12
        svg.rect((width - tw) / 2, y + 4, tw, 16, "#FFFFFF", Muted, 3, 1)
        svg.text(width / 2, y + 16, t.name, "middle", 11, bold = true, fill = Muted)
      ctx.region(t.line, left, y, width - left, rowH)
      continue
    svg.text(left, y + 16, t.name, size = 12)
    let x = gx + float(t.start) * dayW
    if t.milestone:
      let cx = x
      let cy = y + rowH / 2
      svg.polygon([(cx, cy - 7), (cx + 7, cy), (cx, cy + 7), (cx - 7, cy)], Ink, Ink)
      ctx.region(t.line, cx - 8, y, 16 + nameW, rowH)
    else:
      let w = max(2.0, float(t.duration) * dayW - 2)
      let fill = if t.color.len > 0: t.color else: "#B9CCE8"
      svg.rect(x + 1, y + 5, w, rowH - 10, fill, Accent, 3)
      if t.complete > 0:
        svg.rect(x + 1, y + 5, w * float(min(t.complete, 100)) / 100, rowH - 10, Accent, Accent, 3)
        svg.text(x + w + 6, y + 16, $t.complete & " %", size = 10, fill = Muted)
      ctx.region(t.line, left, y, x + w - left + 2, rowH)
  for (a, b) in links:
    let ta = tasks[a]
    let tb = tasks[b]
    let x0 = gx + float(ta.start + ta.duration) * dayW
    let y0 = rowOf[a] + rowH / 2
    let x1 = gx + float(tb.start) * dayW
    let y1 = rowOf[b] + rowH / 2
    svg.path("M" & fmtF(x0) & " " & fmtF(y0) & " H" & fmtF(x0 + 6) & " V" & fmtF(y1) & " H" & fmtF(x1), "none", Muted, 1)
    svg.arrowHead(x1, y1, 0, "filled", Muted)
  (svg, width, height)
