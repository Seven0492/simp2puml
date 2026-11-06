## Préprocesseur PlantUML, sous-ensemble sûr et borné : !define, !definelong, !undef,
## variables, !if/!ifdef/!ifndef, !while, !procedure, !function et quelques fonctions
## pures. Il travaille uniquement sur le texte du diagramme : aucun fichier, réseau,
## environnement ni date n'est consulté, et !include reste refusé par `preprocess.nim`.
import std/[strutils, tables]
import common, themes

const
  MaxOutputLines = 200_000
  MaxLineLength = 1 shl 20
  MaxCallDepth = 50
  MaxLoopTurns = 10_000
  MaxPasses = 16

type
  Macro = object
    params: seq[(string, string)]  ## nom, valeur par défaut (vide si absente)
    hasDefault: seq[bool]
    body: seq[SrcLine]
    isFunction, isDefine: bool
  Engine = object
    ctx: Context
    frames: seq[Table[string, string]]  ## 0 : global ; un cadre par appel
    macros: Table[string, Macro]
    output: seq[SrcLine]
    depth: int
    refused: seq[string]
    overflow: bool
    turns: int  ## tours de boucle de tout le rendu : des !while imbriqués se multiplient

  Ctrl = enum ctrlNext, ctrlReturn

proc tooBig(e: var Engine, line: int) =
  if not e.overflow:
    e.overflow = true
    e.ctx.error(line, "préprocesseur : expansion trop grande, arrêtée")

proc getVar(e: Engine, name: string): (bool, string) =
  for i in countdown(e.frames.high, 0):
    if e.frames[i].hasKey(name): return (true, e.frames[i][name])
  (false, "")

proc isWord(c: char): bool = c in IdentChars or c == '$'

proc findClose(s: string, open: int): int =
  ## Parenthèse fermante, hors guillemets ; -1 si absente.
  var depth = 0
  var quote = false
  for i in open ..< s.len:
    if s[i] == '"': quote = not quote
    elif not quote:
      if s[i] == '(': inc depth
      elif s[i] == ')':
        dec depth
        if depth == 0: return i
  -1

proc splitArgs(s: string): seq[string] =
  var depth = 0
  var quote = false
  var cur = ""
  for c in s:
    if c == '"': quote = not quote
    if not quote:
      if c == '(': inc depth
      elif c == ')': dec depth
      elif c == ',' and depth == 0:
        result.add cur.strip()
        cur = ""
        continue
    cur.add c
  if cur.strip().len > 0 or result.len > 0: result.add cur.strip()

proc call(e: var Engine, name: string, args: seq[string], line: int): string
proc evaluate(e: var Engine, src: string, line: int): string

const Refused = ["%getenv", "%load_json", "%filename", "%dirpath", "%file_exists", "%feature", "%date",
  "%version", "%get_json_keys", "%str2json", "%function_exists", "%variable_exists", "%invoke_procedure", "%call_user_func"]

proc builtin(e: var Engine, name: string, args: seq[string], line: int): (bool, string) =
  template a(i: int): string = (if i < args.len: e.evaluate(args[i], line) else: "")
  case name
  of "%strlen": (true, $a(0).len)
  of "%upper": (true, a(0).toUpperAscii())
  of "%lower": (true, a(0).toLowerAscii())
  of "%true": (true, "1")
  of "%false": (true, "0")
  of "%not": (true, if a(0) in ["", "0", "false"]: "1" else: "0")
  of "%newline": (true, "\\n")
  of "%string": (true, a(0))
  of "%intval":
    try: (true, $parseInt(a(0).strip())) except ValueError: (true, "0")
  of "%strpos": (true, $a(0).find(a(1)))
  of "%substr":
    let s = a(0)
    var start, n = 0
    try: start = parseInt(a(1)) except ValueError: discard
    try: n = (if args.len > 2: parseInt(a(2)) else: s.len) except ValueError: discard
    start = clamp(start, 0, s.len)
    (true, s[start ..< min(s.len, start + max(0, n))])
  else: (false, "")

proc substitute(e: var Engine, s: string, line: int): string =
  ## Une passe : variables, fonctions et macros, sur des mots entiers.
  var i = 0
  while i < s.len:
    let c = s[i]
    let prev = if i > 0: s[i - 1] else: ' '
    # une variable se remplace même collée à un mot (« C$i ») ; le reste, sur des mots entiers
    if c == '$' or ((c == '%' or c in IdentStartChars) and not isWord(prev)):
      var j = i + 1
      while j < s.len and s[j] in IdentChars: inc j
      let word = s[i ..< j]
      if c == '%' and j < s.len and s[j] == '(':
        let close = findClose(s, j)
        if close > 0:
          if word in Refused:
            if word notin e.refused:
              e.refused.add word
              e.ctx.warn(line, "fonction du préprocesseur refusée (accès extérieur) : " & word)
            i = close + 1
            continue
          let (known, value) = e.builtin(word, splitArgs(s[j + 1 ..< close]), line)
          if known:
            result.add value
            i = close + 1
            continue
      if e.macros.hasKey(word):
        let m = e.macros[word]
        if j < s.len and s[j] == '(':
          let close = findClose(s, j)
          if close > 0:
            result.add e.call(word, splitArgs(s[j + 1 ..< close]), line)
            i = close + 1
            continue
        elif m.params.len == 0:
          result.add e.call(word, @[], line)
          i = j
          continue
      if c == '$':
        let (found, value) = e.getVar(word)
        if found:
          result.add value
          i = j
          continue
      result.add word
      i = j
      continue
    result.add c
    inc i
    if result.len > MaxLineLength:
      e.tooBig(line)
      return s

proc expandText(e: var Engine, s: string, line: int): string =
  result = s
  for _ in 1 .. MaxPasses:
    let next = e.substitute(result, line)
    if next == result or e.overflow: return
    result = next
  e.ctx.warn(line, "préprocesseur : expansion interrompue (macros qui s'appellent en boucle ?)")

# ------------------------------------------------------------------ expressions

type Parser = object
  s: string
  pos: int
  line, depth: int

proc skip(p: var Parser) =
  while p.pos < p.s.len and p.s[p.pos] in {' ', '\t'}: inc p.pos

proc isNumber(s: string): bool =
  if s.len == 0: return false
  try: discard parseInt(s.strip()); true except ValueError: false

proc truthy(s: string): bool = s.strip() notin ["", "0", "false"]

proc parseOr(e: var Engine, p: var Parser): string

proc parseAtom(e: var Engine, p: var Parser): string =
  p.skip()
  if p.pos >= p.s.len: return ""
  inc p.depth
  defer: dec p.depth
  if p.depth > MaxNesting: tooDeep(p.line)
  let c = p.s[p.pos]
  if c == '(':
    inc p.pos
    result = e.parseOr(p)
    p.skip()
    if p.pos < p.s.len and p.s[p.pos] == ')': inc p.pos
  elif c == '!':
    inc p.pos
    result = if truthy(e.parseAtom(p)): "0" else: "1"
  elif c == '"' or c == '\'':
    let close = p.s.find(c, p.pos + 1)
    let stop = if close < 0: p.s.len else: close
    result = e.expandText(p.s[p.pos + 1 ..< stop], p.line)
    p.pos = min(p.s.len, stop + 1)
  else:
    let start = p.pos
    if c == '-': inc p.pos
    while p.pos < p.s.len and (p.s[p.pos] in IdentChars or p.s[p.pos] in {'$', '%', '.'}): inc p.pos
    if p.pos < p.s.len and p.s[p.pos] == '(':
      let close = findClose(p.s, p.pos)
      p.pos = if close < 0: p.s.len else: close + 1
    if p.pos == start: inc p.pos  # caractère imprévu : avancer pour ne pas boucler
    result = e.expandText(p.s[start ..< p.pos], p.line)

proc parseMul(e: var Engine, p: var Parser): string =
  result = e.parseAtom(p)
  while true:
    p.skip()
    if p.pos >= p.s.len or p.s[p.pos] notin {'*', '/', '%'}: return
    let op = p.s[p.pos]
    inc p.pos
    let right = e.parseAtom(p)
    if not (result.isNumber and right.isNumber): return
    let (a, b) = (parseInt(result.strip()), parseInt(right.strip()))
    result = case op
      of '*': (if abs(a) < 2_000_000_000 and abs(b) < 2_000_000_000: $(a * b) else: "0")
      of '/': (if b == 0 or b == -1: "0" else: $(a div b))
      else: (if b == 0 or b == -1: "0" else: $(a mod b))

proc parseAdd(e: var Engine, p: var Parser): string =
  result = e.parseMul(p)
  while true:
    p.skip()
    if p.pos >= p.s.len or p.s[p.pos] notin {'+', '-'}: return
    let op = p.s[p.pos]
    inc p.pos
    let right = e.parseMul(p)
    if result.isNumber and right.isNumber:
      let (a, b) = (parseInt(result.strip()), parseInt(right.strip()))
      if abs(a) < 1_000_000_000_000_000 and abs(b) < 1_000_000_000_000_000:
        result = if op == '+': $(a + b) else: $(a - b)
    elif op == '+':
      result.add right
    if result.len > MaxLineLength:
      e.tooBig(p.line)
      return ""

proc parseCompare(e: var Engine, p: var Parser): string =
  result = e.parseAdd(p)
  p.skip()
  for op in ["==", "!=", "<=", ">=", "<", ">"]:
    if p.s.continuesWith(op, p.pos):
      p.pos += op.len
      let right = e.parseAdd(p)
      var outcome: bool
      if result.isNumber and right.isNumber:
        let (a, b) = (parseInt(result.strip()), parseInt(right.strip()))
        outcome = case op
          of "==": a == b
          of "!=": a != b
          of "<=": a <= b
          of ">=": a >= b
          of "<": a < b
          else: a > b
      else:
        outcome = case op
          of "==": result == right
          of "!=": result != right
          of "<=": result <= right
          of ">=": result >= right
          of "<": result < right
          else: result > right
      return if outcome: "1" else: "0"

proc parseOr(e: var Engine, p: var Parser): string =
  result = e.parseCompare(p)
  while true:
    p.skip()
    if p.s.continuesWith("&&", p.pos):
      p.pos += 2
      let right = e.parseCompare(p)
      result = if truthy(result) and truthy(right): "1" else: "0"
    elif p.s.continuesWith("||", p.pos):
      p.pos += 2
      let right = e.parseCompare(p)
      result = if truthy(result) or truthy(right): "1" else: "0"
    else: return

proc evaluate(e: var Engine, src: string, line: int): string =
  var p = Parser(s: src, line: line)
  e.parseOr(p)

# ------------------------------------------------------------------ exécution

proc run(e: var Engine, lines: seq[SrcLine], fixedLine: int, returned: var string): Ctrl

proc directive(t: string): string =
  ## Mot-clé d'une ligne « !mot … », en minuscules.
  var j = 1
  while j < t.len and t[j] in IdentChars: inc j
  t[1 ..< j].toLowerAscii()

proc blockEnd(lines: seq[SrcLine], start: int, opens, closes: openArray[string]): int =
  var depth = 1
  var i = start
  while i < lines.len:
    let t = lines[i].text.strip()
    if t.startsWith("!"):
      var word = directive(t)
      if word == "end" and t.len > 4:  # « !end if », « !end procedure »
        let parts = t[1 .. ^1].splitWhitespace()
        if parts.len >= 2: word = "end" & parts[1].toLowerAscii()
      if word in opens: inc depth
      elif word in closes:
        dec depth
        if depth == 0: return i
    inc i
  -1

proc call(e: var Engine, name: string, args: seq[string], line: int): string =
  if e.depth >= MaxCallDepth:
    e.ctx.error(line, "préprocesseur : appels imbriqués trop profonds (" & name & ")")
    e.overflow = true
    return ""
  let m = e.macros[name]
  var frame = initTable[string, string]()
  var values: seq[string]
  for i, (param, default) in m.params:
    let value = if i < args.len: (if m.isDefine: args[i] else: e.evaluate(args[i], line))
                elif m.hasDefault[i]: e.evaluate(default, line)
                else: ""
    values.add value
    frame[param] = value
  if m.isDefine:
    # !define : substitution textuelle des paramètres, sur des mots entiers
    for l in m.body:
      var text = ""
      var i = 0
      while i < l.text.len:
        if isWord(l.text[i]) and (i == 0 or not isWord(l.text[i - 1])):
          var j = i
          while j < l.text.len and isWord(l.text[j]): inc j
          let w = l.text[i ..< j]
          text.add frame.getOrDefault(w, w)
          i = j
        else:
          text.add l.text[i]
          inc i
      if m.body.len == 1: return text
      result.add (if result.len > 0: "\n" else: "") & text
    if m.body.len > 1:
      for part in result.split('\n'):
        e.output.add SrcLine(num: line, text: e.expandText(part, line))
      return ""
    return result
  inc e.depth
  e.frames.add frame
  var returned = ""
  discard e.run(m.body, line, returned)
  discard e.frames.pop()
  dec e.depth
  returned

proc defineMacro(e: var Engine, header: string, body: seq[SrcLine], isDefine, isFunction: bool, line: int) =
  var name = header.strip()
  var m = Macro(isDefine: isDefine, isFunction: isFunction, body: body)
  let open = name.find('(')
  if open >= 0:
    let close = findClose(name, open)
    if close < 0:
      e.ctx.warn(line, "préprocesseur : parenthèse non fermée : " & header)
      return
    for arg in splitArgs(name[open + 1 ..< close]):
      let eq = arg.find('=')
      if eq >= 0:
        m.params.add (arg[0 ..< eq].strip(), arg[eq + 1 .. ^1].strip())
        m.hasDefault.add true
      else:
        m.params.add (arg, "")
        m.hasDefault.add false
    name = name[0 ..< open].strip()
  e.macros[name] = m

proc run(e: var Engine, lines: seq[SrcLine], fixedLine: int, returned: var string): Ctrl =
  var i = 0
  while i < lines.len and not e.overflow:
    let l = lines[i]
    let num = if fixedLine > 0: fixedLine else: l.num
    let t = l.text.strip()
    inc i
    if not t.startsWith("!") or t.len < 2:
      let text = e.expandText(l.text, num)
      e.output.add SrcLine(num: num, text: text)
      if e.output.len > MaxOutputLines: e.tooBig(num)
      continue
    # variable : « !$x = 1 », « !$x ?= 1 », « !global $x = 1 », « !local $x = 1 »
    var assign = t[1 .. ^1].strip()
    var target = 0  # 0 : cadre courant, 1 : global
    if assign.toLowerAscii().startsWith("global "): (assign = assign[7 .. ^1].strip(); target = 1)
    elif assign.toLowerAscii().startsWith("local "): assign = assign[6 .. ^1].strip()
    if assign.startsWith("$"):
      let eq = assign.find('=')
      if eq > 0:
        let weak = assign[eq - 1] == '?'
        let name = assign[0 ..< (if weak: eq - 1 else: eq)].strip()
        if not weak or not e.getVar(name)[0]:
          let value = e.evaluate(assign[eq + 1 .. ^1], num)
          if target == 1: e.frames[0][name] = value
          else: e.frames[^1][name] = value
        continue
    var word = directive(t)
    let args = t[1 + word.len .. ^1].strip()
    if word == "unquoted" and args.len > 0:
      word = args.splitWhitespace()[0].toLowerAscii()
    case word
    of "define":
      let parts = args.splitWhitespace(1)
      if parts.len == 0: continue
      # l'en-tête va jusqu'à la parenthèse fermante s'il y a des paramètres
      var header = parts[0]
      var value = if parts.len > 1: parts[1] else: ""
      let open = args.find('(')
      if open >= 0 and open <= parts[0].len:
        let close = findClose(args, open)
        if close > 0:
          header = args[0 .. close]
          value = args[close + 1 .. ^1].strip()
      e.defineMacro(header, @[SrcLine(num: num, text: value)], true, false, num)
    of "definelong":
      let close = blockEnd(lines, i, ["definelong"], ["enddefinelong"])
      if close < 0:
        e.ctx.error(num, "!definelong sans !enddefinelong")
        return ctrlNext
      e.defineMacro(args, lines[i ..< close], true, false, num)
      i = close + 1
    of "undef", "undefine":
      e.macros.del(args.strip())
    of "procedure", "function":
      let afterKeyword = if t[1 .. ^1].toLowerAscii().startsWith("unquoted"): args.splitWhitespace(1) else: @["", args]
      let header = if afterKeyword.len > 1: afterKeyword[1] else: ""
      let oneLine = header.find("!return")
      if oneLine > 0:
        e.defineMacro(header[0 ..< oneLine], @[SrcLine(num: num, text: header[oneLine .. ^1])], false, true, num)
        continue
      let close = blockEnd(lines, i, ["procedure", "function"],
        ["endprocedure", "endfunction", "endfunc", "endproc"])
      if close < 0:
        e.ctx.error(num, "!" & word & " non fermée")
        return ctrlNext
      e.defineMacro(header, lines[i ..< close], false, word == "function", num)
      i = close + 1
    of "return":
      returned = e.evaluate(args, num)
      return ctrlReturn
    of "if", "ifdef", "ifndef":
      let close = blockEnd(lines, i, ["if", "ifdef", "ifndef"], ["endif"])
      if close < 0:
        e.ctx.error(num, "!" & word & " sans !endif")
        return ctrlNext
      # branches : (condition vraie, début, fin)
      var chosen: seq[SrcLine]
      var taken = false
      var condition = case word
        of "ifdef": e.macros.hasKey(args.strip()) or e.getVar(args.strip())[0]
        of "ifndef": not (e.macros.hasKey(args.strip()) or e.getVar(args.strip())[0])
        else: truthy(e.evaluate(args, num))
      var start = i
      var depth = 0
      var k = i
      while k <= close:
        let tk = lines[k].text.strip()
        let w = if tk.startsWith("!"): directive(tk) else: ""
        if w in ["if", "ifdef", "ifndef"]: inc depth
        elif w == "endif" and k < close: dec depth
        if depth == 0 and (k == close or w in ["else", "elseif"]):
          if condition and not taken:
            chosen = lines[start ..< k]
            taken = true
          if w == "else": condition = true
          elif w == "elseif": condition = truthy(e.evaluate(tk[1 + w.len .. ^1], num))
          start = k + 1
        inc k
      i = close + 1
      if e.run(chosen, fixedLine, returned) == ctrlReturn: return ctrlReturn
    of "while":
      let close = blockEnd(lines, i, ["while"], ["endwhile"])
      if close < 0:
        e.ctx.error(num, "!while sans !endwhile")
        return ctrlNext
      let body = lines[i ..< close]
      while not e.overflow and truthy(e.evaluate(args, num)):
        inc e.turns
        if e.turns > MaxLoopTurns:
          e.ctx.error(num, "!while : plus de " & $MaxLoopTurns & " tours, arrêté")
          e.overflow = true
          break
        if e.run(body, fixedLine, returned) == ctrlReturn: return ctrlReturn
      i = close + 1
    of "theme":
      let parts = args.splitWhitespace()
      if parts.len >= 2 and parts[1].toLowerAscii() == "from":
        e.ctx.warn(num, "!theme … from : thème extérieur refusé (jamais chargé) : " & args)
      elif parts.len == 0 or not applyTheme(parts[0].strip(chars = {'"'})):
        e.ctx.warn(num, "thème inconnu : " & args)
    of "log", "assert", "dump_memory":
      discard
    of "else", "elseif", "endif", "endwhile", "endprocedure", "endfunction", "enddefinelong":
      e.ctx.warn(num, "!" & word & " sans ouverture correspondante")
    else:
      # !include, !import, !pragma… : signalés par `handleCommon`
      e.output.add SrcLine(num: num, text: l.text)
  ctrlNext

proc expandMacros*(lines: seq[SrcLine], ctx: Context): seq[SrcLine] =
  var e = Engine(ctx: ctx)
  e.frames.add initTable[string, string]()
  var returned = ""
  discard e.run(lines, 0, returned)
  e.output
