## Diagrammes d'activité (nouvelle syntaxe) : actions, conditions, boucles,
## parallélisme, partitions, notes et étiquettes de flèches.
import std/[strutils, math]
import common, preprocess

type
  AKind = enum akSeq, akAction, akStart, akStop, akEnd, akIf, akWhile, akRepeat, akFork,
    akPartition, akDetach, akConnector, akLane
  ANode = ref object
    kind: AKind
    text: string
    line: int
    color: string
    children: seq[ANode]           ## akSeq
    notes: seq[(int, string, string, int)] ## (index de l'enfant, côté, texte, ligne) pour akSeq
    labels: seq[string]            ## étiquette sur la flèche après l'enfant i
    labelLines: seq[int]
    branchLines: seq[int]          ## ligne de chaque branche (else, elseif, case…)
    endLine: int                   ## ligne qui ferme : repeat while, endwhile
    branches: seq[ANode]           ## akIf / akFork : séquences
    branchLabels: seq[string]
    body: ANode                    ## akWhile / akRepeat / akPartition
    cond, yes, no: string
    merge: bool                    ## fork : end merge
    # mise en page
    w, h, axis, boff: float
    terminal: bool

proc newSeqNode(): ANode = ANode(kind: akSeq)

proc parenText(s: string): (string, string) =
  ## Extrait le premier (...) équilibré et renvoie (contenu, reste).
  let a = s.find('(')
  if a < 0: return ("", s)
  var depth = 0
  for i in a ..< s.len:
    if s[i] == '(': inc depth
    elif s[i] == ')':
      dec depth
      if depth == 0: return (s[a + 1 ..< i], s[i + 1 .. ^1])
  (s[a + 1 .. ^1], "")

proc parseActivity(doc: Doc, ctx: Context): ANode =
  let root = newSeqNode()
  type Frame = object
    node: ANode      # nœud structurant (if/while/…) ou nil pour la racine
    cur: ANode       # séquence courante
    line: int
  var stack: seq[Frame] = @[Frame(node: nil, cur: root)]
  template cur(): ANode = stack[^1].cur
  proc push(f: Frame) =
    if stack.len >= MaxNesting: tooDeep(f.line)
    stack.add f
  proc add(n: ANode) =
    cur.children.add n
    cur.labels.add ""
    cur.labelLines.add 0
  let lines = doc.lines
  var i = 0
  while i < lines.len:
    let l = lines[i]
    var t = l.text.strip()
    inc i
    if t.len == 0: continue
    var lw = t.toLowerAscii()
    var color = ""
    if t.startsWith("#") and t.contains(':'):
      let c = t.find(':')
      color = colorValue(t[0 ..< c])
      t = t[c .. ^1]
      lw = t.toLowerAscii()
    if t.startsWith(":"):
      # action (éventuellement multiligne) jusqu'à ; | < > / ] }
      var buf = t[1 .. ^1]
      const Ends = {';', '|', '<', '>', '/', ']', '}'}
      while not (buf.len > 0 and buf[^1] in Ends) and i < lines.len:
        buf.add "\n" & lines[i].text.strip()
        inc i
      if buf.len > 0 and buf[^1] in Ends: buf.setLen(buf.len - 1)
      else: ctx.error(l.num, "action sans « ; » final")
      add ANode(kind: akAction, text: unescapeLabel(buf), line: l.num, color: color)
      continue
    if lw == "start": (add ANode(kind: akStart, line: l.num); continue)
    if lw == "stop": (add ANode(kind: akStop, line: l.num, terminal: true); continue)
    if lw == "end": (add ANode(kind: akEnd, line: l.num, terminal: true); continue)
    if lw in ["detach", "kill", "break"]: (add ANode(kind: akDetach, line: l.num, terminal: true); continue)
    if t.startsWith("|") and t.endsWith("|") and t.len > 2:
      var name = t.strip(chars = {'|'})
      if name.startsWith("#"):
        let p = name.find('|')
        if p >= 0: name = name[p + 1 .. ^1]
      add ANode(kind: akLane, text: name, line: l.num)
      continue
    if t.startsWith("(") and t.endsWith(")") and t.len <= 6:
      add ANode(kind: akConnector, text: t[1 .. ^2], line: l.num)
      continue
    if t.startsWith("->") or t.startsWith("-[") or t.startsWith("-->"):
      var lbl = t
      let a = lbl.find('>')
      lbl = if a >= 0: lbl[a + 1 .. ^1].strip() else: ""
      if lbl.endsWith(";"): lbl.setLen(lbl.len - 1)
      if cur.labels.len > 0:
        cur.labels[^1] = unescapeLabel(lbl.strip())
        cur.labelLines[^1] = l.num
      continue
    if lw.startsWith("if") and (lw.startsWith("if (") or lw.startsWith("if(")):
      let (cond, rest) = parenText(t[2 .. ^1])
      var label = ""
      let r = rest.strip()
      if r.toLowerAscii().startsWith("then"):
        label = parenText(r)[0]
      elif r.toLowerAscii().startsWith("is"):
        label = parenText(r)[0]
      let n = ANode(kind: akIf, cond: unescapeLabel(cond), line: l.num)
      let br = newSeqNode()
      n.branches.add br
      n.branchLabels.add unescapeLabel(label)
      n.branchLines.add l.num
      add n
      push Frame(node: n, cur: br, line: l.num)
      continue
    if lw.startsWith("elseif"):
      if stack[^1].node == nil or stack[^1].node.kind != akIf:
        ctx.error(l.num, "elseif sans if")
        continue
      let (cond, rest) = parenText(t[6 .. ^1])
      let lbl = parenText(rest)[0]
      let br = newSeqNode()
      stack[^1].node.branches.add br
      stack[^1].node.branchLabels.add unescapeLabel(cond & (if lbl.len > 0: " → " & lbl else: ""))
      stack[^1].node.branchLines.add l.num
      stack[^1].cur = br
      continue
    if lw.startsWith("else"):
      if stack[^1].node == nil or stack[^1].node.kind != akIf:
        ctx.error(l.num, "else sans if")
        continue
      let br = newSeqNode()
      stack[^1].node.branches.add br
      stack[^1].node.branchLabels.add unescapeLabel(parenText(t[4 .. ^1])[0])
      stack[^1].node.branchLines.add l.num
      stack[^1].cur = br
      continue
    if lw.startsWith("endif") or lw == "end if":
      if stack[^1].node == nil or stack[^1].node.kind != akIf:
        ctx.error(l.num, "endif sans if")
        continue
      discard stack.pop()
      continue
    if lw.startsWith("switch"):
      let n = ANode(kind: akIf, cond: unescapeLabel(parenText(t)[0]), line: l.num)
      add n
      push Frame(node: n, cur: newSeqNode(), line: l.num)
      continue
    if lw.startsWith("case"):
      if stack[^1].node == nil or stack[^1].node.kind != akIf:
        ctx.error(l.num, "case hors d'un switch")
        continue
      let br = newSeqNode()
      stack[^1].node.branches.add br
      stack[^1].node.branchLabels.add unescapeLabel(parenText(t)[0])
      stack[^1].node.branchLines.add l.num
      stack[^1].cur = br
      continue
    if lw.startsWith("endswitch"):
      if stack[^1].node == nil or stack[^1].node.kind != akIf:
        ctx.error(l.num, "endswitch sans switch")
        continue
      discard stack.pop()
      continue
    if lw.startsWith("while"):
      let (cond, rest) = parenText(t[5 .. ^1])
      let n = ANode(kind: akWhile, cond: unescapeLabel(cond), line: l.num, body: newSeqNode())
      let r = rest.strip()
      if r.toLowerAscii().startsWith("is"): n.yes = unescapeLabel(parenText(r)[0])
      add n
      push Frame(node: n, cur: n.body, line: l.num)
      continue
    if lw.startsWith("endwhile") or lw.startsWith("end while"):
      if stack[^1].node == nil or stack[^1].node.kind != akWhile:
        ctx.error(l.num, "endwhile sans while")
        continue
      stack[^1].node.no = unescapeLabel(parenText(t)[0])
      stack[^1].node.endLine = l.num
      discard stack.pop()
      continue
    if lw == "repeat" or lw.startsWith("repeat :") or (lw.startsWith("repeat") and not lw.startsWith("repeat while") and not lw.startsWith("repeatwhile")):
      let n = ANode(kind: akRepeat, line: l.num, body: newSeqNode())
      add n
      push Frame(node: n, cur: n.body, line: l.num)
      let after = t[6 .. ^1].strip()
      if after.startsWith(":"):
        var a = after[1 .. ^1]
        if a.endsWith(";"): a.setLen(a.len - 1)
        n.body.children.add ANode(kind: akAction, text: unescapeLabel(a), line: l.num)
        n.body.labels.add ""
        n.body.labelLines.add 0
      continue
    if lw.startsWith("repeat while") or lw.startsWith("repeatwhile"):
      if stack[^1].node == nil or stack[^1].node.kind != akRepeat:
        ctx.error(l.num, "repeat while sans repeat")
        continue
      let n = stack[^1].node
      let (cond, rest) = parenText(t)
      n.cond = unescapeLabel(cond)
      n.endLine = l.num
      let r = rest.strip()
      let rl = r.toLowerAscii()
      if rl.startsWith("is"):
        let (y, rest2) = parenText(r)
        n.yes = unescapeLabel(y)
        let r2 = rest2.strip()
        if r2.toLowerAscii().startsWith("not"): n.no = unescapeLabel(parenText(r2)[0])
      elif rl.startsWith("not"):
        n.no = unescapeLabel(parenText(r)[0])
      discard stack.pop()
      continue
    if lw == "backward" or lw.startsWith("backward"):
      ctx.info(l.num, "backward : action de retour affichée dans la boucle")
      if lw.contains(":"):
        var a = t[t.find(':') + 1 .. ^1]
        if a.endsWith(";"): a.setLen(a.len - 1)
        add ANode(kind: akAction, text: unescapeLabel(a), line: l.num)
      continue
    if lw in ["fork", "split"]:
      let n = ANode(kind: akFork, line: l.num, text: lw)
      let br = newSeqNode()
      n.branches.add br
      n.branchLabels.add ""
      n.branchLines.add l.num
      add n
      push Frame(node: n, cur: br, line: l.num)
      continue
    if lw in ["fork again", "split again"]:
      if stack[^1].node == nil or stack[^1].node.kind != akFork:
        ctx.error(l.num, lw & " sans fork/split")
        continue
      let br = newSeqNode()
      stack[^1].node.branches.add br
      stack[^1].node.branchLabels.add ""
      stack[^1].node.branchLines.add l.num
      stack[^1].cur = br
      continue
    if lw.startsWith("end fork") or lw.startsWith("endfork") or lw.startsWith("end split") or
        lw.startsWith("end merge") or lw.startsWith("endsplit"):
      if stack[^1].node == nil or stack[^1].node.kind != akFork:
        ctx.error(l.num, "fin de fork/split sans ouverture")
        continue
      stack[^1].node.merge = lw.contains("merge")
      discard stack.pop()
      continue
    if lw.startsWithWord("partition") or lw.startsWithWord("group") or lw.startsWithWord("package") or
        lw.startsWithWord("rectangle") or lw.startsWithWord("card"):
      var name = t.afterWord(t.split({' ', '\t'})[0])
      if name.endsWith("{"): name = name[0 .. ^2].strip()
      name = stripColor(name)[0]
      let n = ANode(kind: akPartition, text: unescapeLabel(stripQuotes(name)), line: l.num, body: newSeqNode())
      add n
      push Frame(node: n, cur: n.body, line: l.num)
      continue
    if t == "}" or lw == "end group" or lw == "end partition":
      if stack[^1].node == nil or stack[^1].node.kind != akPartition:
        ctx.error(l.num, "accolade fermante sans partition")
        continue
      discard stack.pop()
      continue
    if lw.startsWithWord("note") or lw.startsWithWord("floating"):
      var rest = t
      if lw.startsWithWord("floating"): rest = rest.afterWord("floating")
      rest = rest.afterWord("note")
      var side = "right"
      if rest.toLowerAscii().startsWithWord("left"): (side = "left"; rest = rest.afterWord("left"))
      elif rest.toLowerAscii().startsWithWord("right"): rest = rest.afterWord("right")
      rest = stripColor(rest)[0]
      var text = ""
      if rest.startsWith(":"):
        text = rest[1 .. ^1].strip()
      else:
        var parts: seq[string]
        var closed = false
        while i < lines.len:
          let s = lines[i].text
          inc i
          if s.strip().toLowerAscii() in ["end note", "endnote"]:
            closed = true
            break
          parts.add s.strip()
        if not closed: ctx.error(l.num, "note sans « end note »")
        text = parts.join("\n")
      let idx = cur.children.high
      cur.notes.add (idx, side, unescapeLabel(text), l.num)
      continue
    ctx.warn(l.num, "syntaxe d'activité non reconnue, ligne ignorée : " & t)
  for k in countdown(stack.high, 1):
    let f = stack[k]
    let name = case f.node.kind
      of akIf: (if f.node.branches.len > 0 and f.node.cond.len > 0: "if" else: "switch")
      of akWhile: "while"
      of akRepeat: "repeat"
      of akFork: "fork"
      of akPartition: "partition"
      else: "bloc"
    let closing = case f.node.kind
      of akIf: "endif"
      of akWhile: "endwhile"
      of akRepeat: "repeat while"
      of akFork: "end fork"
      of akPartition: "}"
      else: "fin"
    ctx.error(f.line, "« " & name & " » non fermé (« " & closing & " » manquant)")
  root

# ------------------------------------------------------------------ mise en page

const Gap = 26.0
const BranchGap = 34.0
const Diamond = 13.0

proc measure(n: ANode)

proc measureSeq(n: ANode) =
  var left, right = 0.0
  var h = 0.0
  for i, c in n.children:
    measure(c)
    var r = c.w - c.axis
    for (idx, side, text, _) in n.notes:
      if idx == i:
        let lines = text.split('\n')
        let nw = blockWidth(lines) + 22
        if side == "right": r = max(r, c.w - c.axis + 18 + nw)
        else: left = max(left, c.axis + 18 + nw)
    left = max(left, c.axis)
    right = max(right, r)
    if n.labels.len > i and n.labels[i].len > 0:
      right = max(right, textWidth(n.labels[i], 12) + 12)
    h += c.h
    if i < n.children.high: h += Gap + (if n.labels[i].len > 0: 6.0 else: 0.0)
  for (idx, side, text, _) in n.notes:
    if idx < 0:
      let lines = text.split('\n')
      right = max(right, blockWidth(lines) + 30)
  n.axis = left
  n.w = left + right
  n.h = h
  if n.children.len > 0: n.terminal = n.children[^1].terminal

proc measure(n: ANode) =
  case n.kind
  of akSeq: measureSeq(n)
  of akAction:
    let lines = n.text.split('\n')
    n.w = blockWidth(lines) + 26
    n.h = blockHeight(lines.len) + 16
    n.axis = n.w / 2
  of akStart: (n.w = 22; n.h = 22; n.axis = 11)
  of akStop, akEnd: (n.w = 24; n.h = 24; n.axis = 12)
  of akDetach: (n.w = 14; n.h = 10; n.axis = 7)
  of akConnector: (n.w = 26; n.h = 26; n.axis = 13)
  of akLane:
    n.w = textWidth(n.text, 12, true) + 20
    n.h = 20
    n.axis = n.w / 2
  of akIf, akFork:
    var total = 0.0
    var hmax = 0.0
    for i, b in n.branches:
      measure(b)
      let lw = if n.branchLabels.len > i: textWidth(n.branchLabels[i], 11) + 8 else: 0.0
      b.w = max(b.w, max(lw, 20.0))
      if b.children.len == 0: (b.axis = b.w / 2)
      total += b.w
      if i > 0: total += BranchGap
      hmax = max(hmax, b.h)
    if n.branches.len == 0:
      total = 30
    let top = if n.kind == akIf: 2 * Diamond + 22 else: 26.0
    let bottom = if n.kind == akIf or n.merge: 2 * Diamond + 18 else: 26.0
    n.h = top + hmax + bottom
    # axe : milieu entre premier et dernier axe de branche
    var x = 0.0
    var axes: seq[float]
    for i, b in n.branches:
      axes.add x + b.axis
      x += b.w + BranchGap
    let axis0 = if axes.len > 0: (axes[0] + axes[^1]) / 2 else: 15.0
    let halfCond = if n.kind == akIf: (textWidth(n.cond, 12) + 2 * Diamond + 12) / 2 else: 0.0
    let left = max(axis0, halfCond)
    n.boff = left - axis0
    n.axis = left
    n.w = max(n.boff + total, left + halfCond)
    if n.kind == akIf and n.branches.len == 1: n.w += 24
    n.terminal = n.branches.len > 0
    for b in n.branches:
      if not b.terminal: n.terminal = false
    if n.kind == akIf and n.branches.len == 1: n.terminal = false
  of akWhile:
    measure(n.body)
    let condW = textWidth(n.cond, 12) + 2 * Diamond + 10
    n.axis = max(n.body.axis + 30, condW / 2 + 20)
    n.w = n.axis + max(n.body.w - n.body.axis + 30, max(condW / 2 + 30, Diamond + 24 + textWidth(n.no, 11)))
    n.h = 2 * Diamond + Gap + n.body.h + Gap + 18
  of akRepeat:
    measure(n.body)
    let condW = textWidth(n.cond, 12) + 2 * Diamond + 10
    n.axis = max(n.body.axis + 30, condW / 2 + 20)
    n.w = n.axis + max(n.body.w - n.body.axis + 20, condW / 2 + 20 + textWidth(n.yes, 11))
    n.h = 2 * Diamond + Gap + n.body.h + Gap + 2 * Diamond + 4
  of akPartition:
    measure(n.body)
    let tw = textWidth(n.text, 12, true) + 20
    n.w = max(n.body.w + 28, tw + 20)
    n.axis = (n.w - n.body.w) / 2 + n.body.axis
    n.h = n.body.h + 46
    n.terminal = n.body.terminal

proc arrowDown(svg: var Svg, x, y0, y1: float) =
  svg.line(x, y0, x, y1, arrowStroke())
  svg.arrowHead(x, y1, PI / 2, "filled", arrowStroke())

proc diamond(svg: var Svg, cx, cy: float) =
  styled("activitydiamond"):
    svg.polygon([(cx, cy - Diamond), (cx + Diamond, cy), (cx, cy + Diamond), (cx - Diamond, cy)], HeaderFill)

proc draw(n: ANode, svg: var Svg, x, y: float, ctx: Context)

proc drawSeq(n: ANode, svg: var Svg, x, y: float, ctx: Context) =
  var yy = y
  let ax = x + n.axis
  for i, c in n.children:
    let cxl = ax - c.axis
    draw(c, svg, cxl, yy, ctx)
    for (idx, side, text, noteLine) in n.notes:
      if idx == i:
        let lines = text.split('\n')
        let nw = blockWidth(lines) + 22
        let nh = blockHeight(lines.len) + 12
        let nx = if side == "right": cxl + c.w + 18 else: cxl - 18 - nw
        let ny = yy + c.h / 2 - nh / 2
        svg.note(nx, ny, nw, nh)
        svg.textBlock(nx + 10, ny + 6, lines)
        ctx.region(noteLine, nx, ny, nw, nh)
        svg.line((if side == "right": cxl + c.w else: cxl), yy + c.h / 2,
          (if side == "right": nx else: nx + nw), yy + c.h / 2, NoteStroke, 1, "4 3")
    yy += c.h
    if i < n.children.high:
      let extra = if n.labels[i].len > 0: 6.0 else: 0.0
      if not c.terminal:
        # la flèche renvoie à son étiquette, sinon à ce vers quoi elle pointe
        let target = if n.labelLines[i] > 0: n.labelLines[i] else: n.children[i + 1].line
        svg.beginLink(ctx, target)
        svg.arrowDown(ax, yy, yy + Gap + extra)
        ctx.link(target, [(ax, yy), (ax, yy + Gap + extra)])
        svg.endLink(target)
        if n.labels[i].len > 0:
          svg.text(ax + 6, yy + Gap / 2 + 6, n.labels[i], size = 12, fill = Muted)
          ctx.region(n.labelLines[i], ax + 4, yy + Gap / 2 - 6, textWidth(n.labels[i], 12) + 4, 16)
      yy += Gap + extra
  for (idx, side, text, noteLine) in n.notes:
    if idx < 0:
      let lines = text.split('\n')
      svg.note(x + 4, y, blockWidth(lines) + 22, blockHeight(lines.len) + 12)
      ctx.region(noteLine, x + 4, y, blockWidth(lines) + 22, blockHeight(lines.len) + 12)
      svg.textBlock(x + 14, y + 6, lines)

proc draw(n: ANode, svg: var Svg, x, y: float, ctx: Context) =
  case n.kind
  of akSeq: drawSeq(n, svg, x, y, ctx)
  of akAction:
    styled("activity"):
      let fill = if n.color.len > 0: n.color else: Fill
      svg.rect(x, y, n.w, n.h, fill, Stroke, 10)
      svg.textBlock(x + 13, y + 8, n.text.split('\n'))
    ctx.region(n.line, x, y, n.w, n.h)
  of akStart:
    svg.circle(x + 11, y + 11, 10, Ink, Ink)
    ctx.region(n.line, x, y, n.w, n.h)
  of akStop:
    svg.circle(x + 12, y + 12, 11, Background, Ink, 1.4)
    svg.circle(x + 12, y + 12, 6.5, Ink, Ink)
    ctx.region(n.line, x, y, n.w, n.h)
  of akEnd:
    svg.circle(x + 12, y + 12, 11, Background, Ink, 1.4)
    svg.line(x + 6, y + 6, x + 18, y + 18, Ink, 1.6)
    svg.line(x + 18, y + 6, x + 6, y + 18, Ink, 1.6)
    ctx.region(n.line, x, y, n.w, n.h)
  of akDetach:
    svg.line(x, y + 5, x + 14, y + 5, Ink, 2)
    ctx.region(n.line, x, y, n.w, n.h)
  of akConnector:
    svg.circle(x + 13, y + 13, 12, Fill)
    svg.text(x + 13, y + 17.5, n.text, "middle", 12, bold = true)
    ctx.region(n.line, x, y, n.w, n.h)
  of akLane:
    svg.rect(x, y, n.w, n.h, HeaderFill, Muted, 10, 1)
    svg.text(x + n.w / 2, y + 14, n.text, "middle", 12, bold = true, fill = Muted)
    ctx.region(n.line, x, y, n.w, n.h)
  of akIf, akFork:
    let ax = x + n.axis
    var bx = x + n.boff
    var axes: seq[float]
    var lefts: seq[float]
    for b in n.branches:
      lefts.add bx
      axes.add bx + b.axis
      bx += b.w + BranchGap
    let top = if n.kind == akIf: 2 * Diamond + 22 else: 26.0
    let bottomY = y + n.h
    let mergeY = if n.kind == akIf or n.merge: bottomY - Diamond else: bottomY - 13
    if n.kind == akIf:
      svg.diamond(ax, y + Diamond)
      if n.cond.len > 0:
        svg.text(ax + Diamond + 6, y + Diamond - 4, n.cond, size = 12)
        ctx.region(n.line, ax + Diamond + 4, y + Diamond - 16, textWidth(n.cond, 12) + 4, 16)
      ctx.region(n.line, ax - Diamond, y, 2 * Diamond, 2 * Diamond)
    else:
      let x0 = if axes.len > 0: min(axes[0], ax) - 16 else: ax - 20
      let x1 = if axes.len > 0: max(axes[^1], ax) + 16 else: ax + 20
      svg.rect(x0, y, x1 - x0, 7, Ink, Ink, 2)
      ctx.region(n.line, x0, y - 2, x1 - x0, 11)
    for i, b in n.branches:
      let a = axes[i]
      let startY = if n.kind == akIf: y + Diamond else: y + 7
      if n.kind == akIf:
        if abs(a - ax) < 0.5:
          svg.line(ax, y + 2 * Diamond, ax, y + top - 2)
        else:
          let sx = if a < ax: ax - Diamond else: ax + Diamond
          svg.line(sx, startY, a, startY)
          svg.line(a, startY, a, y + top - 2)
        svg.arrowHead(a, y + top, PI / 2, "filled")
        if n.branchLabels[i].len > 0:
          let lx = if abs(a - ax) < 0.5: a + 6 else: a + 4
          let ly = startY + 15 + (if abs(a - ax) < 0.5: Diamond else: 0.0)
          svg.text(lx, ly, n.branchLabels[i], size = 11, fill = Muted)
          if i < n.branchLines.len: ctx.region(n.branchLines[i], lx - 2, ly - 11, textWidth(n.branchLabels[i], 11) + 4, 14)
      else:
        svg.arrowDown(a, startY, y + top)
      draw(b, svg, lefts[i], y + top, ctx)
      if not b.terminal:
        let endY = y + top + b.h
        if n.kind == akIf or n.merge:
          if abs(a - ax) < 0.5:
            svg.arrowDown(a, endY, mergeY - Diamond)
          else:
            svg.line(a, endY, a, mergeY)
            let ex = if a < ax: ax - Diamond else: ax + Diamond
            svg.line(a, mergeY, ex, mergeY)
            svg.arrowHead(ex, mergeY, (if a < ax: 0.0 else: PI), "filled")
        else:
          svg.arrowDown(a, endY, mergeY - 4)
    if n.kind == akIf or n.merge:
      svg.diamond(ax, mergeY)
    else:
      let x0 = if axes.len > 0: min(axes[0], ax) - 16 else: ax - 20
      let x1 = if axes.len > 0: max(axes[^1], ax) + 16 else: ax + 20
      svg.rect(x0, mergeY - 4, x1 - x0, 7, Ink, Ink, 2)
    if n.kind == akIf and n.branches.len == 1:
      # branche « else » implicite
      let ex = x + n.w - 4
      svg.line(ax + Diamond, y + Diamond, ex, y + Diamond)
      svg.line(ex, y + Diamond, ex, mergeY)
      svg.line(ex, mergeY, ax + Diamond, mergeY)
      svg.arrowHead(ax + Diamond, mergeY, PI, "filled")
    # sortie
    if n.kind == akIf or n.merge:
      svg.line(ax, mergeY + Diamond, ax, bottomY)
    else:
      svg.line(ax, mergeY + 3, ax, bottomY)
  of akWhile:
    let ax = x + n.axis
    svg.diamond(ax, y + Diamond)
    ctx.region(n.line, ax - Diamond, y, 2 * Diamond, 2 * Diamond)
    if n.cond.len > 0:
      svg.text(ax + Diamond + 6, y + Diamond - 4, n.cond, size = 12)
      ctx.region(n.line, ax + Diamond + 4, y + Diamond - 16, textWidth(n.cond, 12) + 4, 16)
    let by = y + 2 * Diamond + Gap
    svg.arrowDown(ax, y + 2 * Diamond, by)
    if n.yes.len > 0: svg.text(ax + 6, y + 2 * Diamond + 14, n.yes, size = 11, fill = Muted)
    draw(n.body, svg, ax - n.body.axis, by, ctx)
    let be = by + n.body.h
    let lx = x + 8
    if not n.body.terminal:
      svg.path("M" & fmtF(ax) & " " & fmtF(be) & " V" & fmtF(be + 10) & " H" & fmtF(lx) & " V" & fmtF(y + Diamond) &
        " H" & fmtF(ax - Diamond))
      svg.arrowHead(ax - Diamond, y + Diamond, 0, "filled")
    let rx = x + n.w - 8
    svg.path("M" & fmtF(ax + Diamond) & " " & fmtF(y + Diamond) & " H" & fmtF(rx) & " V" & fmtF(y + n.h - 4) &
      " H" & fmtF(ax) & " V" & fmtF(y + n.h))
    if n.no.len > 0:
      svg.text(rx - 4, y + Diamond + 15, n.no, "end", 11, fill = Muted)
      if n.endLine > 0: ctx.region(n.endLine, rx - 6 - textWidth(n.no, 11), y + Diamond + 4, textWidth(n.no, 11) + 4, 14)
  of akRepeat:
    let ax = x + n.axis
    svg.diamond(ax, y + Diamond)
    ctx.region(n.line, ax - Diamond, y, 2 * Diamond, 2 * Diamond)
    let by = y + 2 * Diamond + Gap
    svg.arrowDown(ax, y + 2 * Diamond, by)
    draw(n.body, svg, ax - n.body.axis, by, ctx)
    let be = by + n.body.h
    let dy = be + Gap + Diamond
    if not n.body.terminal: svg.arrowDown(ax, be, dy - Diamond)
    svg.diamond(ax, dy)
    # le bas de la boucle est sa propre ligne : « repeat while »
    if n.endLine > 0: ctx.region(n.endLine, ax - Diamond, dy - Diamond, 2 * Diamond, 2 * Diamond)
    if n.cond.len > 0:
      svg.text(ax + Diamond + 6, dy - 4, n.cond, size = 12)
      if n.endLine > 0: ctx.region(n.endLine, ax + Diamond + 4, dy - 16, textWidth(n.cond, 12) + 4, 16)
    let lx = x + 8
    svg.path("M" & fmtF(ax - Diamond) & " " & fmtF(dy) & " H" & fmtF(lx) & " V" & fmtF(y + Diamond) &
      " H" & fmtF(ax - Diamond))
    svg.arrowHead(ax - Diamond, y + Diamond, 0, "filled")
    if n.yes.len > 0: svg.text(lx + 4, dy - 6, n.yes, size = 11, fill = Muted)
    svg.line(ax, dy + Diamond, ax, y + n.h)
    if n.no.len > 0: svg.text(ax + 6, dy + Diamond + 12, n.no, size = 11, fill = Muted)
  of akPartition:
    svg.rect(x, y, n.w, n.h, "none", Muted, 4, 1.1)
    let tw = textWidth(n.text, 12, true) + 16
    svg.path("M" & fmtF(x) & " " & fmtF(y + 20) & " H" & fmtF(x + tw) & " L" & fmtF(x + tw + 6) & " " &
      fmtF(y + 14) & " V" & fmtF(y), HeaderFill, Muted, 1.1)
    svg.text(x + 8, y + 14, n.text, size = 12, bold = true)
    ctx.region(n.line, x, y, tw + 6, 20)
    let ax = x + n.axis
    svg.line(ax, (if ax <= x + tw + 6: y + 20 else: y), ax, y + 30)
    draw(n.body, svg, (n.w - n.body.w) / 2 + x, y + 32, ctx)
    if not n.body.terminal: svg.line(ax, y + 32 + n.body.h, ax, y + n.h)

proc renderActivity*(doc: Doc, ctx: Context): (Svg, float, float) =
  let root = parseActivity(doc, ctx)
  var svg: Svg
  if root.children.len == 0 and root.notes.len == 0:
    return (svg, 0.0, 0.0)
  measure(root)
  draw(root, svg, 20, 20, ctx)
  (svg, root.w + 40, root.h + 40)
