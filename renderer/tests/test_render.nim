## Tests du moteur de rendu natif : nim c -r --path:src tests/test_render.nim
import std/[unittest, os, strutils, xmlparser, xmltree, osproc]
import pumlrender/[common, render, preprocess]

const Samples = currentSourcePath().parentDir / "samples"

proc errors(r: RenderResult): seq[string] =
  for d in r.diags:
    if d.severity == sevError: result.add $d.line & ": " & d.message

proc warnings(r: RenderResult): seq[string] =
  for d in r.diags:
    if d.severity == sevWarning: result.add $d.line & ": " & d.message

proc validSvg(svg: string): bool =
  try:
    let x = parseXml(svg)
    x.tag == "svg" and x.attr("viewBox").len > 0
  except CatchableError:
    false

suite "échantillons":
  for (file, kind) in [("class.puml", dkClass), ("sequence.puml", dkSequence),
      ("activity.puml", dkActivity), ("state.puml", dkState), ("usecase.puml", dkUsecase),
      ("component.puml", dkComponent), ("object.puml", dkObject), ("mindmap.puml", dkMindmap),
      ("wbs.puml", dkWbs), ("json.puml", dkJson), ("yaml.puml", dkYaml), ("gantt.puml", dkGantt)]:
    test file:
      let r = renderPuml(readFile(Samples / file))
      check r.kind == kind
      check r.errors.len == 0
      check r.warnings.len == 0
      check validSvg(r.svg)
      check r.width > 0 and r.height > 0
      check r.regions.len > 0

suite "diagnostics":
  test "préprocesseur jamais exécuté":
    let r = renderPuml("@startuml\n!include /etc/passwd\n!define X 1\nclass A\n@enduml\n")
    check r.errors.len == 0
    check r.warnings.len == 2
    check r.diags[0].line == 2
    check "passwd" notin r.svg

  test "@enduml manquant":
    let r = renderPuml("@startuml\nA -> B\n")
    check r.errors.len == 1

  test "syntaxe inconnue signalée avec sa ligne":
    let r = renderPuml("@startuml\nclass A\nceci n'est pas du plantuml ???\n@enduml\n")
    check r.warnings.len == 1
    check r.diags[0].line == 3

  test "corps de classe non fermé":
    let r = renderPuml("@startuml\nclass A {\n  x : int\n@enduml\n")
    check r.errors.len == 1
    check r.diags[0].line == 2

  test "JSON invalide":
    let r = renderPuml("@startjson\n{ \"a\": }\n@endjson\n")
    check r.errors.len == 1
    check validSvg(r.svg)

  test "type non pris en charge : aperçu de remplacement":
    let r = renderPuml("@startsalt\n{ Login | \"x\" }\n@endsalt\n")
    check r.kind == dkUnsupported
    check r.errors.len == 0
    check validSvg(r.svg)

  test "texte échappé dans le SVG":
    let r = renderPuml("@startuml\nclass \"<script>&\" as X\n@enduml\n")
    check "<script>" notin r.svg
    check "&lt;script&gt;&amp;" in r.svg

  test "plusieurs blocs":
    let text = "@startuml\nA -> B\n@enduml\n@startmindmap\n* racine\n@endmindmap\n"
    check renderPuml(text).blocks == 2
    check renderPuml(text, 1).kind == dkMindmap

suite "classes et relations":
  test "flèches et têtes":
    let r = renderPuml("@startuml\nA <|-- B\nC *-- D\nE o-- F\nG ..> H : utilise\nI -up-> J\n@enduml\n")
    check r.kind == dkClass
    check r.errors.len == 0
    check r.warnings.len == 0
    check "utilise" in r.svg

  test "relations avec multiplicités et étiquette":
    let r = renderPuml("@startuml\nclass Client\nClient \"1\" --> \"*\" Commande : passe >\n@enduml\n")
    check r.warnings.len == 0
    check "passe" in r.svg

  test "membres hors corps":
    let r = renderPuml("@startuml\nclass A\nA : +nom : String\n@enduml\n")
    check r.warnings.len == 0
    check "nom : String" in r.svg

suite "séquence":
  test "fragments et retours":
    let r = renderPuml("@startuml\nA -> B ++ : appel\nB --> A -- : ok\nopt cas\nA -> A : boucle\nend\nreturn fin\n@enduml\n")
    check r.kind == dkSequence
    check r.errors.len == 0

  test "fragment non fermé":
    let r = renderPuml("@startuml\nA -> B\nloop toujours\nA -> B\n@enduml\n")
    check r.errors.len == 1

suite "gantt":
  test "durée démesurée bornée":
    let r = renderPuml("@startgantt\n[T] lasts 99999999 days\n@endgantt\n")
    check r.errors.len == 0
    check r.warnings.len == 1
    check validSvg(r.svg)
    check r.width < 3660.0 * 17 + 1000

  test "début négatif ou lointain sans plantage":
    let r = renderPuml("@startgantt\n[A] lasts 2 days\n[A] starts D+-2000000\n" &
      "[B] starts D+99999999\n[C] starts at [A]'s start\n@endgantt\n")
    check r.errors.len == 0
    check r.warnings.len == 2
    check validSvg(r.svg)

suite "robustesse (revue)":
  test "conteneur redéclaré dans lui-même : pas de boucle":
    for src in ["package P {\n  package P {\n  }\n}", "package User {\nclass User\n}",
                "node A {\n node A {\n }\n}", "state A {\n state B {\n  state A\n }\n}"]:
      let r = renderPuml("@startuml\n" & src & "\n@enduml\n")
      check validSvg(r.svg)

  test "note à gauche après un retour":
    let r = renderPuml("@startuml\nA -> B ++ : call\nreturn ok\nnote left : fin\n@enduml\n")
    check r.errors.len == 0
    check validSvg(r.svg)

  test "séparateur titré : UTF-8 valide":
    let r = renderPuml("@startuml\nclass A {\n-- Section --\nx : int\n}\n@enduml\n")
    check validSvg(r.svg)
    check "Section" in r.svg
    check "\xC2<" notin r.svg and "\xA7" notin r.svg

  test "élément nommé comme une directive":
    check "Book" in renderPuml("@startuml\nTitle \"1\" -- \"*\" Book\n@enduml\n").svg
    check "Body" in renderPuml("@startuml\nHeader -> Body : x\n@enduml\n").svg
    let t = renderPuml("@startuml\ntitle Mon titre\nclass A\n@enduml\n")
    check "Mon titre" in t.svg

  test "générique et implements":
    let r = renderPuml("@startuml\nclass Foo<T> {\n}\nFoo --> Bar\nclass X extends B implements C, D\n@enduml\n")
    check r.svg.count("Foo") == 1
    check "B implements" notin r.svg
    check validSvg(r.svg)

  test "nombres démesurés sans débordement":
    for src in ["@startuml\nautonumber 9223372036854775807\nA -> B\nA -> B\n@enduml\n",
                "@startgantt\n[A] lasts -2000000000000000000 weeks\n@endgantt\n",
                "@startgantt\n[A] lasts 9000000000000000000 weeks\n@endgantt\n"]:
      let r = renderPuml(src)
      check validSvg(r.svg)
      check r.errors.len == 0

  test "BOM, caractères interdits en XML et UTF-8 invalide":
    check renderPuml("\xEF\xBB\xBF@startuml\nA -> B\n@enduml\n").errors.len == 0
    let r = renderPuml("@startuml\nA -> B : x\uFFFF\xFFy\n@enduml\n")
    check validSvg(r.svg)
    check "\uFFFD" in r.svg

  test "bloc skinparam non fermé signalé":
    let r = renderPuml("@startuml\nskinparam class {\nBackgroundColor red\nclass A\n@enduml\n")
    check r.errors.len == 1

suite "ligne de commande":
  const Bin = currentSourcePath().parentDir.parentDir / "bin" / "pumlrender"
  test "refuse d'écraser une sortie":
    if not fileExists(Bin):
      skip()
    else:
      let dir = getTempDir() / "pumlrender-test-" & $getCurrentProcessId()
      createDir(dir)
      defer: removeDir(dir)
      let src = dir / "a.puml"
      let dst = dir / "a.svg"
      writeFile(src, "@startuml\nA -> B\n@enduml\n")
      writeFile(dst, "existant")
      let (_, code) = execCmdEx(quoteShell(Bin) & " " & quoteShell(src) & " -o " & quoteShell(dst))
      check code == 2
      check readFile(dst) == "existant"
      removeFile(dst)
      let (_, code2) = execCmdEx(quoteShell(Bin) & " " & quoteShell(src) & " -o " & quoteShell(dst))
      check code2 == 0
      check validSvg(readFile(dst))

  test "protocole éditeur":
    if not fileExists(Bin):
      skip()
    else:
      let (outp, code) = execCmdEx(quoteShell(Bin) & " --editor", input = "@startuml\nA -> B\n@enduml\n")
      check code == 0
      check outp.startsWith("PUMLRENDER 1\nKIND\tsequence\n")
      check "\nSVG\t" in outp

  test "mode persistant : plusieurs requêtes sur un même processus":
    if not fileExists(Bin):
      skip()
    else:
      let a = "@startuml\nA -> B\n@enduml\n"
      let b = "@startmindmap\n* x\n@endmindmap\n"
      let input = "RENDER 1 0 " & $a.len & "\n" & a & "RENDER 2 0 " & $b.len & "\n" & b
      let (outp, code) = execCmdEx(quoteShell(Bin) & " --serve", input = input)
      check code == 0
      check outp.count("PUMLRENDER 1") == 2
      check "ID\t1\nKIND\tsequence" in outp
      check "ID\t2\nKIND\tmindmap" in outp

  test "mode persistant : une requête invalide n'arrête pas le serveur":
    if not fileExists(Bin):
      skip()
    else:
      let a = "@startuml\nA -> B\n@enduml\n"
      let input = "RENDER 7 x 3\n" & "RENDER  9 0 " & $a.len & "\n" & a
      let (outp, code) = execCmdEx(quoteShell(Bin) & " --serve", input = input)
      check code == 0
      check "ID\t7\nERROR" in outp
      check "ID\t9\nKIND\tsequence" in outp

  test "sortie : un lien symbolique n'est jamais suivi ni remplacé":
    if not fileExists(Bin) or not defined(posix):
      skip()
    else:
      let dir = getTempDir() / "pumlrender-lien-" & $getCurrentProcessId()
      createDir(dir)
      defer: removeDir(dir)
      writeFile(dir / "a.puml", "@startuml\nA -> B\n@enduml\n")
      createSymlink(dir / "absent.svg", dir / "lien.svg")
      let (_, code) = execCmdEx(quoteShell(Bin) & " " & quoteShell(dir / "a.puml") & " -o " & quoteShell(dir / "lien.svg"))
      check code == 2
      check not fileExists(dir / "absent.svg")
