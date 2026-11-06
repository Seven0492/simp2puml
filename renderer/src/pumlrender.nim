## pumlrender : rendu SVG natif de PlantUML, sans Java ni préprocesseur.
##
##   pumlrender [options] [fichier.puml]
##
## Sans fichier, lit l'entrée standard. Le SVG est écrit sur la sortie
## standard, ou dans le fichier indiqué par -o (jamais écrasé).
import std/[os, strutils, monotimes]
when defined(posix):
  import std/posix
  var O_NOFOLLOW {.importc, header: "<fcntl.h>".}: cint
import pumlrender/[common, render]

const Version* = "0.1.0"
const MaxInput = 20 * 1024 * 1024

const Usage = """pumlrender """ & Version & """ — rendu SVG natif de diagrammes PlantUML

Usage : pumlrender [options] [fichier]

Options :
  -o, --output=FICHIER   écrire le SVG dans FICHIER (refusé s'il existe)
  -b, --block=N          bloc @start…@end à rendre (1 par défaut)
      --check            n'afficher que les diagnostics
      --editor           protocole de l'éditeur (diagnostics, régions, SVG)
      --serve            mode persistant pour l'éditeur (requêtes sur l'entrée)
  -h, --help             afficher cette aide
  -v, --version          afficher la version

Le préprocesseur PlantUML (!include, !define…) n'est jamais exécuté.
Code de sortie : 0 sans erreur, 1 si le diagramme contient des erreurs,
2 pour une erreur d'utilisation ou d'entrée/sortie."""

proc clean(s: string): string = s.multiReplace(("\t", " "), ("\n", " "), ("\r", " "))

proc editorOutput(r: RenderResult, id = ""): string =
  result = "PUMLRENDER 1\n"
  if id.len > 0: result.add "ID\t" & id & "\n"
  result.add "KIND\t" & $r.kind & "\n"
  result.add "BLOCKS\t" & $r.blocks & "\n"
  result.add "SIZE\t" & fmtF(r.width) & "\t" & fmtF(r.height) & "\n"
  for d in r.diags:
    result.add "DIAG\t" & $d.line & "\t" & $d.severity & "\t" & clean(d.message) & "\n"
  for g in r.regions:
    result.add "REGION\t" & $g.line & "\t" & fmtF(g.x) & "\t" & fmtF(g.y) & "\t" & fmtF(g.w) & "\t" & fmtF(g.h) & "\n"
  result.add "SVG\t" & $r.svg.len & "\n"
  result.add r.svg

proc serve(): int =
  ## Mode persistant (comme un serveur de langage) : une requête par message.
  ##   requête : « RENDER <id> <bloc> <octets>\n » suivi du texte
  ##   réponse : le protocole --editor précédé de « ID\t<id> »
  ## Une requête invalide reçoit une réponse ERROR ; le serveur continue.
  proc reply(id, msg: string) =
    stdout.write "PUMLRENDER 1\n" & (if id.len > 0: "ID\t" & clean(id) & "\n" else: "") & "ERROR\t" & clean(msg) & "\n"
    stdout.flushFile()
  var header: string
  while stdin.readLine(header):
    let parts = header.splitWhitespace()
    if parts.len != 4 or parts[0] != "RENDER":
      reply("", "requête invalide")
      continue
    var blockIdx, size: int
    try:
      blockIdx = parseInt(parts[2])
      size = parseInt(parts[3])
    except ValueError:
      # longueur inconnue : impossible de sauter le texte, on répond et continue
      reply(parts[1], "requête invalide (nombre attendu)")
      continue
    if size < 0:
      reply(parts[1], "longueur négative")
      continue
    if size > MaxInput:
      # lire et jeter le texte pour rester synchronisé
      var left = size
      var buf = newString(65536)
      while left > 0:
        let n = stdin.readBuffer(addr buf[0], min(left, buf.len))
        if n <= 0: return 2
        left -= n
      reply(parts[1], "entrée trop volumineuse (maximum : 20 MiB)")
      continue
    var data = newString(size)
    if size > 0 and stdin.readBuffer(addr data[0], size) != size: return 2
    stdout.write editorOutput(renderPuml(data, max(0, blockIdx)), parts[1])
    stdout.flushFile()
  0

proc readBounded(f: File): string =
  ## Lit au plus MaxInput + 1 octets (pour détecter un dépassement sans tout lire).
  result = newString(MaxInput + 1)
  var n = 0
  while n < result.len:
    let r = f.readBuffer(addr result[n], result.len - n)
    if r <= 0: break
    n += r
  result.setLen(n)

proc writeNew(output, content: string) =
  ## Écrit une nouvelle sortie sans jamais écraser ni suivre de lien : fichier
  ## temporaire créé en exclusivité, puis lien dur (échoue si la sortie existe).
  when defined(posix):
    proc fail(what: string) = raise newException(IOError, what & " : " & $strerror(errno))
    proc writeAll(fd: cint) =
      var off = 0
      while off < content.len:
        let w = posix.write(fd, unsafeAddr content[off], content.len - off)
        if w <= 0: fail(output)
        off += w
      discard fsync(fd)
    let flags = O_WRONLY or O_CREAT or O_EXCL or O_NOFOLLOW
    let tmp = output & ".tmp" & $getCurrentProcessId() & "-" & $getMonoTime().ticks
    let fd = posix.open(tmp.cstring, flags, 0o644)
    if fd < 0: fail(tmp)
    try: writeAll(fd)
    finally: discard posix.close(fd)
    let linked = link(tmp.cstring, output.cstring)
    let err = errno
    discard unlink(tmp.cstring)
    if linked == 0: return
    if err == EEXIST: raise newException(IOError, "la sortie existe déjà : " & output)
    if err notin [EPERM, EOPNOTSUPP]: errno = err; fail(output)
    # système de fichiers sans liens durs : création exclusive directe
    let fd2 = posix.open(output.cstring, flags, 0o644)
    if fd2 < 0: fail(output)
    try: writeAll(fd2)
    finally: discard posix.close(fd2)
  else:
    if fileExists(output): raise newException(IOError, "la sortie existe déjà : " & output)
    writeFile(output, content)

proc main(): int =
  var input = ""
  var output = ""
  var blockIdx = 0
  var check, editor = false
  let args = commandLineParams()
  var k = 0
  proc value(name: string, inline: string): string =
    if inline.len > 0: return inline
    inc k
    if k >= args.len:
      raise newException(ValueError, "l'option " & name & " attend une valeur")
    args[k]
  try:
    while k < args.len:
      let a = args[k]
      var name = a
      var inline = ""
      if a.startsWith("--") and a.contains('='):
        name = a[0 ..< a.find('=')]
        inline = a[a.find('=') + 1 .. ^1]
      case name
      of "-o", "--output": output = value(name, inline)
      of "-b", "--block":
        try: blockIdx = clamp(parseInt(value(name, inline)), 1, high(int32)) - 1
        except ValueError: raise newException(ValueError, "--block attend un entier")
      of "--check": check = true
      of "--editor": editor = true
      of "--serve": return serve()
      of "-h", "--help":
        echo Usage
        return 0
      of "-v", "--version":
        echo "pumlrender " & Version
        return 0
      else:
        if a.startsWith("-") and a != "-":
          raise newException(ValueError, "option inconnue " & a)
        if input.len > 0:
          raise newException(ValueError, "un seul fichier d'entrée est accepté")
        input = a
      inc k
  except ValueError as e:
    stderr.writeLine "Erreur : " & e.msg
    return 2
  var text = ""
  try:
    if input.len == 0 or input == "-":
      text = readBounded(stdin)
    else:
      var f: File
      if not open(f, input): raise newException(IOError, input)
      try: text = readBounded(f)
      finally: close(f)
  except IOError, OSError:
    stderr.writeLine "Erreur : lecture impossible : " & getCurrentExceptionMsg()
    return 2
  if text.len > MaxInput:
    stderr.writeLine "Erreur : entrée trop volumineuse (maximum : 20 MiB)"
    return 2
  if text.startsWith("\xEF\xBB\xBF"): text = text[3 .. ^1]
  let r = renderPuml(text, max(0, blockIdx))
  if editor:
    stdout.write editorOutput(r)
    return 0
  var errors = false
  for d in r.diags:
    if d.severity == sevError: errors = true
    if d.severity != sevInfo or check:
      stderr.writeLine (if input.len > 0: input else: "<stdin>") & ":" & $d.line & ": " & $d.severity & ": " & d.message
  if check:
    return (if errors: 1 else: 0)
  if output.len > 0:
    if fileExists(output) or dirExists(output) or symlinkExists(output):
      stderr.writeLine "Erreur : la sortie existe déjà : " & output
      return 2
    try:
      writeNew(output, r.svg)
    except IOError, OSError:
      stderr.writeLine "Erreur : écriture impossible : " & getCurrentExceptionMsg()
      return 2
  else:
    stdout.write r.svg
  if errors: 1 else: 0

when isMainModule:
  quit(main())
