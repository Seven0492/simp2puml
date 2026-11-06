## Découpage des blocs @start/@end, commentaires, directives communes et
## détection du type de diagramme. Le préprocesseur PlantUML (!include,
## !define, !theme…) n'est jamais exécuté : ses lignes sont signalées et ignorées.
import std/[strutils]
import common

type
  DiagramKind* = enum
    dkClass = "class", dkSequence = "sequence", dkActivity = "activity",
    dkState = "state", dkUsecase = "usecase", dkComponent = "component",
    dkObject = "object", dkMindmap = "mindmap", dkWbs = "wbs", dkGantt = "gantt",
    dkJson = "json", dkYaml = "yaml", dkTiming = "timing", dkUnsupported = "unsupported",
    dkEmpty = "empty"

  Doc* = object
    kind*: DiagramKind
    tag*: string           ## uml, mindmap, json…
    meta*: Meta
    lines*: seq[SrcLine]   ## lignes utiles (sans commentaires ni directives communes)
    raw*: seq[SrcLine]     ## lignes du bloc, commentaires retirés (pour json/yaml)
    startLine*: int

  Block* = object
    tag*: string
    startLine*: int
    lines*: seq[SrcLine]

const UnsupportedTags* = ["salt", "ditaa", "dot", "math", "latex", "regex", "ebnf",
  "nwdiag", "wire", "chart", "chronology", "files", "board", "creole", "chen", "def", "flow", "sdl", "git"]

proc findBlocks*(text: string): seq[Block] =
  var cur: Block
  var inBlock = false
  let rawLines = text.replace("\r\n", "\n").replace('\r', '\n').split('\n')
  for i, raw in rawLines:
    let t = raw.strip()
    let lower = t.toLowerAscii()
    if not inBlock:
      if lower.startsWith("@start"):
        var tag = ""
        var j = 6
        while j < lower.len and lower[j] in IdentChars:
          tag.add lower[j]
          inc j
        cur = Block(tag: tag, startLine: i + 1)
        inBlock = true
    else:
      if lower.startsWith("@end"):
        result.add cur
        inBlock = false
      else:
        cur.lines.add SrcLine(num: i + 1, text: raw)
  if inBlock:
    cur.tag = cur.tag & "!"  # bloc non terminé
    result.add cur

proc stripComments(lines: seq[SrcLine]): seq[SrcLine] =
  var inBlock = false
  for l in lines:
    var s = l.text
    var outp = ""
    var i = 0
    while i < s.len:
      if inBlock:
        if s.continuesWith("'/", i):
          inBlock = false
          i += 2
        else: inc i
      elif s.continuesWith("/'", i):
        inBlock = true
        i += 2
      else:
        outp.add s[i]
        inc i
    let t = outp.strip(leading = true, trailing = false)
    if t.startsWith("'"):
      outp = ""
    result.add SrcLine(num: l.num, text: outp.strip(leading = false, trailing = true))

proc detectKind(lines: seq[SrcLine]): DiagramKind =
  var score: array[DiagramKind, int]
  var arrows = 0
  for l in lines:
    let t = l.text.strip()
    if t.len == 0: continue
    let lw = t.toLowerAscii()
    template hit(k: DiagramKind, n: int) = score[k] += n
    if (t.startsWith(":") and (t.endsWith(";") or t.endsWith("|") or t.endsWith(">") or t.endsWith("]") or t.endsWith("}") or t.endsWith("/"))) or
        lw in ["start", "stop", "end", "detach", "kill", "fork", "fork again", "end fork", "repeat",
               "endif", "endwhile", "split", "split again", "end split", "endswitch", "end merge"] or
        lw.startsWith("if (") or lw.startsWith("if(") or lw.startsWith("while (") or lw.startsWith("while(") or
        lw.startsWith("repeat while") or lw.startsWith("switch (") or lw.startsWith("elseif") or
        lw.startsWith("partition ") or (t.startsWith("|") and t.endsWith("|") and t.len > 2):
      hit(dkActivity, 3)
    if t.startsWith("(*)") or t.contains("--> (*)") or t.contains("-> (*)"): hit(dkActivity, 4)
    if t.contains("[*]") or lw.startsWith("state ") or lw.startsWith("[h]") : hit(dkState, 4)
    for kw in ["class ", "abstract class ", "abstract ", "interface ", "enum ", "annotation ",
               "protocol ", "struct ", "exception ", "metaclass ", "stereotype ", "circle ", "diamond "]:
      if lw.startsWith(kw): hit(dkClass, 4)
    for kw in ["object ", "map "]:
      if lw.startsWith(kw): hit(dkObject, 4)
    for kw in ["usecase ", "usecase/ ", "actor/ ", "business "]:
      if lw.startsWith(kw): hit(dkUsecase, 4)
    if lw.startsWith("(") and not lw.startsWith("(*)") and not lw.startsWith("()"): hit(dkUsecase, 2)
    if t.startsWith(":") and t.find(':', 1) > 0 and not t.endsWith(";"): hit(dkUsecase, 2)
    if lw.startsWith("actor "): (hit(dkUsecase, 1); hit(dkSequence, 1))
    for kw in ["component ", "node ", "cloud ", "artifact ", "folder ", "file ", "frame ",
               "storage ", "card ", "agent ", "stack ", "hexagon ", "person ", "port ",
               "portin ", "portout ", "() ", "interface ", "rectangle "]:
      if lw.startsWith(kw): hit(dkComponent, 2)
    if lw.startsWith("[") and not lw.startsWith("[*]") and not lw.startsWith("[h"): hit(dkComponent, 3)
    for kw in ["participant ", "boundary ", "control ", "entity ", "collections ", "queue ",
               "database "]:
      if lw.startsWith(kw): (hit(dkSequence, 2); hit(dkComponent, 1))
    for kw in ["activate ", "deactivate ", "destroy ", "autonumber", "alt ", "alt", "loop ", "opt ",
               "par", "critical", "break", "group ", "ref over", "note over", "return", "create ",
               "box ", "end box", "newpage", "autoactivate", "hnote", "rnote", "...", "|||"]:
      if lw == kw.strip() or lw.startsWith(kw): hit(dkSequence, 3)
    if lw.startsWith("==") and lw.endsWith("=="): hit(dkSequence, 2)
    for kw in ["robust ", "concise ", "clock ", "binary "]:
      if lw.startsWith(kw): hit(dkTiming, 5)
    if t.startsWith("@") and t.len > 1 and t[1] in Digits: hit(dkTiming, 2)
    if not lw.startsWith("..."):
      for a in ["<|", "|>", "*--", "--*", " o--", "--o ", "..>", "<..", "-[hidden]-", "#--", "--#", "x--", "--x "]:
        if t.contains(a):
          hit(dkClass, 3)
          break
      if t.contains("..") and not t.contains("...") and not t.startsWith("("): hit(dkClass, 1)
    if t.contains("->") or t.contains("<-"):
      if not t.contains("--") and not t.contains(".."): inc arrows
  if arrows > 0: score[dkSequence] += 1
  var best = dkEmpty
  var bestScore = 0
  for k in [dkActivity, dkClass, dkSequence, dkState, dkUsecase, dkComponent, dkObject, dkTiming]:
    if score[k] > bestScore:
      best = k
      bestScore = score[k]
  if best == dkEmpty:
    var any = false
    for l in lines:
      if l.text.strip().len > 0: any = true
    if any:
      best = if arrows > 0: dkSequence else: dkClass
  best

const DirectiveWords = ["title", "header", "footer", "caption", "legend", "page", "left", "right",
  "center", "scale", "skin", "skinparam", "set", "hide", "show", "mainframe", "style", "pragma"]

proc relationNamedLikeDirective(t: string): bool =
  ## La ligne commence par un mot de directive mais est une relation :
  ## mot, multiplicité facultative entre guillemets, puis une flèche.
  let sp = t.find({' ', '\t'})
  if sp <= 0 or t[0 ..< sp].toLowerAscii() notin DirectiveWords: return false
  var r = t[sp .. ^1].strip()
  if r.startsWith("\""):
    let e = r.find('"', 1)
    if e < 0: return false
    r = r[e + 1 .. ^1].strip()
  let tokEnd = r.find({' ', '\t'})
  let tok = if tokEnd < 0: r else: r[0 ..< tokEnd]
  tok.len >= 2 and tok[0] in {'-', '.', '<', '*', 'o', '|', '}', '#', '+', 'x', '^'} and
    (tok.contains("--") or tok.contains("->") or tok.contains("<-") or tok.contains(".."))

proc handleCommon(lines: seq[SrcLine], ctx: Context, meta: var Meta, keepMultiline: bool): seq[SrcLine] =
  ## Retire titre, en-têtes, légende, skinparam et directives.
  var i = 0
  while i < lines.len:
    let l = lines[i]
    let t = l.text.strip()
    let lw = t.toLowerAscii()
    proc collect(endTag: string): string =
      var parts: seq[string]
      inc i
      while i < lines.len and lines[i].text.strip().toLowerAscii() notin [endTag, endTag.replace(" ", "")]:
        parts.add lines[i].text.strip()
        inc i
      if i >= lines.len: ctx.warn(l.num, "bloc « " & endTag & " » manquant")
      parts.join("\n")
    if t.len == 0:
      result.add l
    elif t.startsWith("!"):
      ctx.warn(l.num, "directive de préprocesseur ignorée (jamais exécutée) : " & t.split({' ', '\t'})[0])
    elif relationNamedLikeDirective(t):
      # « Title "1" -- "*" Book », « Header -> Body » : un élément, pas une directive
      result.add l
    elif lw == "title":
      meta.titleLine = l.num
      meta.title = collect("end title")
    elif lw.startsWithWord("title"):
      meta.titleLine = l.num
      meta.title = unescapeLabel(t.afterWord("title"))
    elif lw in ["header", "left header", "right header", "center header"]:
      meta.header = collect("endheader")
    elif lw.startsWithWord("header") or lw.startsWith("left header ") or lw.startsWith("right header ") or lw.startsWith("center header "):
      meta.header = unescapeLabel(t[t.toLowerAscii().find("header") + 6 .. ^1].strip())
    elif lw in ["footer", "left footer", "right footer", "center footer"]:
      meta.footer = collect("endfooter")
    elif lw.startsWithWord("footer") or lw.startsWith("left footer ") or lw.startsWith("right footer ") or lw.startsWith("center footer "):
      meta.footer = unescapeLabel(t[t.toLowerAscii().find("footer") + 6 .. ^1].strip())
    elif lw.startsWithWord("caption"):
      meta.caption = unescapeLabel(t.afterWord("caption"))
    elif lw.startsWithWord("legend"):
      meta.legendLine = l.num
      meta.legendAlign = t.afterWord("legend").toLowerAscii()
      meta.legend = collect("endlegend")
    elif lw.startsWithWord("skinparam"):
      if t.endsWith("{"):
        inc i
        while i < lines.len and lines[i].text.strip() != "}": inc i
        if i >= lines.len:
          ctx.error(l.num, "bloc skinparam non fermé (« } » manquant) : la suite du diagramme est ignorée")
      ctx.info(l.num, "skinparam ignoré : le rendu natif utilise son propre style")
    elif lw.startsWithWord("hide") or lw.startsWithWord("show") or lw.startsWithWord("scale") or
        lw.startsWithWord("page") or lw.startsWithWord("allowmixing") or lw.startsWithWord("allow_mixing") or
        lw.startsWithWord("set") and lw.contains("separator") or lw.startsWithWord("mainframe") or
        lw.startsWithWord("skin") or lw.startsWithWord("pragma") or lw.startsWith("<style>") or lw == "style":
      if lw.startsWith("<style>"):
        while i < lines.len and not lines[i].text.contains("</style>"): inc i
        if i >= lines.len:
          ctx.error(l.num, "bloc <style> non fermé (« </style> » manquant) : la suite du diagramme est ignorée")
      elif not (lw.startsWithWord("hide") or lw.startsWithWord("show")):
        ctx.info(l.num, "directive graphique ignorée : " & t)
      else:
        result.add l  # hide/show peut être interprété par certains rendus
    elif lw == "left to right direction":
      meta.leftToRight = true
    elif lw == "top to bottom direction":
      meta.leftToRight = false
    else:
      result.add l
    inc i
  discard keepMultiline

proc prepare*(text: string, ctx: Context, blockIndex = 0): Doc =
  let blocks = findBlocks(text)
  var blk: Block
  if blocks.len == 0:
    let lines = text.replace("\r\n", "\n").replace('\r', '\n').split('\n')
    var any = false
    for i, l in lines:
      blk.lines.add SrcLine(num: i + 1, text: l)
      if l.strip().len > 0: any = true
    blk.tag = "uml"
    blk.startLine = 1
    if any: ctx.warn(1, "@startuml manquant : le texte entier est interprété")
  else:
    let idx = max(0, min(blockIndex, blocks.high))
    blk = blocks[idx]
    if blocks.len > 1:
      ctx.info(blk.startLine, "bloc " & $(idx + 1) & " sur " & $blocks.len & " affiché")
  if blk.tag.endsWith("!"):
    blk.tag = blk.tag[0 .. ^2]
    ctx.error(blk.startLine, "@end" & blk.tag & " manquant")
  result.tag = blk.tag
  result.startLine = blk.startLine
  let cleaned = stripComments(blk.lines)
  result.raw = cleaned
  case blk.tag
  of "uml", "":
    result.lines = handleCommon(cleaned, ctx, result.meta, true)
    result.kind = detectKind(result.lines)
  of "mindmap":
    result.lines = handleCommon(cleaned, ctx, result.meta, true)
    result.kind = dkMindmap
  of "wbs":
    result.lines = handleCommon(cleaned, ctx, result.meta, true)
    result.kind = dkWbs
  of "gantt":
    result.lines = handleCommon(cleaned, ctx, result.meta, true)
    result.kind = dkGantt
  of "json":
    result.kind = dkJson
    result.lines = cleaned
  of "yaml":
    result.kind = dkYaml
    result.lines = blk.lines
  else:
    result.lines = cleaned
    result.kind = dkUnsupported
    if blk.tag notin UnsupportedTags:
      ctx.warn(blk.startLine, "type de bloc inconnu : @start" & blk.tag)
