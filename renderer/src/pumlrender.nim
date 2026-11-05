## pumlrender : rendu SVG natif de PlantUML, sans Java ni préprocesseur.
##
##   pumlrender [options] [fichier.puml]
##
## Sans fichier, lit l'entrée standard. Le SVG est écrit sur la sortie
## standard, ou dans le fichier indiqué par -o (jamais écrasé).
import std/[os, strutils]
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
  -h, --help             afficher cette aide
  -v, --version          afficher la version

Le préprocesseur PlantUML (!include, !define…) n'est jamais exécuté.
Code de sortie : 0 sans erreur, 1 si le diagramme contient des erreurs,
2 pour une erreur d'utilisation ou d'entrée/sortie."""

proc clean(s: string): string = s.multiReplace(("\t", " "), ("\n", " "), ("\r", " "))

proc editorOutput(r: RenderResult): string =
  result = "PUMLRENDER 1\n"
  result.add "KIND\t" & $r.kind & "\n"
  result.add "BLOCKS\t" & $r.blocks & "\n"
  result.add "SIZE\t" & fmtF(r.width) & "\t" & fmtF(r.height) & "\n"
  for d in r.diags:
    result.add "DIAG\t" & $d.line & "\t" & $d.severity & "\t" & clean(d.message) & "\n"
  for g in r.regions:
    result.add "REGION\t" & $g.line & "\t" & fmtF(g.x) & "\t" & fmtF(g.y) & "\t" & fmtF(g.w) & "\t" & fmtF(g.h) & "\n"
  result.add "SVG\t" & $r.svg.len & "\n"
  result.add r.svg

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
        try: blockIdx = parseInt(value(name, inline)) - 1
        except ValueError: raise newException(ValueError, "--block attend un entier")
      of "--check": check = true
      of "--editor": editor = true
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
      text = stdin.readAll()
    else:
      if getFileSize(input) > MaxInput:
        stderr.writeLine "Erreur : fichier trop volumineux (maximum : 20 MiB)"
        return 2
      text = readFile(input)
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
    if fileExists(output) or dirExists(output):
      stderr.writeLine "Erreur : la sortie existe déjà : " & output
      return 2
    try:
      let tmp = output & ".tmp" & $getCurrentProcessId()
      writeFile(tmp, r.svg)
      if fileExists(output):
        removeFile(tmp)
        stderr.writeLine "Erreur : la sortie existe déjà : " & output
        return 2
      moveFile(tmp, output)
    except IOError, OSError:
      stderr.writeLine "Erreur : écriture impossible : " & getCurrentExceptionMsg()
      return 2
  else:
    stdout.write r.svg
  if errors: 1 else: 0

when isMainModule:
  quit(main())
