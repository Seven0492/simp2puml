## Tests du moteur de rendu natif : nim c -r --path:src tests/test_render.nim
import std/[unittest, os, strutils, xmlparser, xmltree, osproc, sequtils]
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

suite "imbrication extrême : diagnostic, jamais d'arrêt du processus":
  proc rep(s: string, n: int): string =
    for _ in 1 .. n: result.add s

  test "activité":
    for line in ["if (a) then\n", "while (a)\n", "partition x {\n", "repeat\n", "fork\n"]:
      let r = renderPuml("@startuml\nstart\n" & rep(line, 50_000) & "@enduml\n")
      check r.errors.anyIt("imbrication" in it)
      check validSvg(r.svg)

  test "conteneurs":
    var src = "@startuml\n"
    for i in 0 ..< 5_000: src.add "package P" & $i & " {\n"
    let r = renderPuml(src & "@enduml\n")
    check r.errors.anyIt("imbrication" in it)
    check validSvg(r.svg)

  test "cartes mentales":
    var src = "@startmindmap\n"
    for i in 1 .. 1_000: src.add rep("*", i) & " x\n"
    let r = renderPuml(src & "@endmindmap\n")
    check r.errors.anyIt("imbrication" in it)
    check validSvg(r.svg)

  test "JSON et YAML":
    check renderPuml("@startjson\n" & rep("[", 100_000) & "\n@endjson\n").errors.anyIt("imbrication" in it)
    check renderPuml("@startyaml\na: " & rep("[", 100_000) & rep("]", 100_000) & "\n@endyaml\n").errors.anyIt("imbrication" in it)

  test "une imbrication raisonnable reste rendue":
    let r = renderPuml("@startuml\nstart\n" & rep("if (a) then\n", 50) & ":x;\n" & rep("endif\n", 50) & "stop\n@enduml\n")
    check r.errors.len == 0

  test "Gantt : longue chaîne de dépendances, sans plantage":
    var src = "@startgantt\n"
    for i in 0 ..< 50_000: src.add "[T" & $i & "] starts at [T" & $(i + 1) & "]'s end\n"
    let r = renderPuml(src & "@endgantt\n")
    check validSvg(r.svg)

  test "Gantt : dépendance circulaire":
    let r = renderPuml("@startgantt\n[A] lasts 1 days\n[B] lasts 1 days\n[A] starts at [B]'s end\n[B] starts at [A]'s end\n@endgantt\n")
    check validSvg(r.svg)

suite "régions JSON et YAML":
  proc lines(r: RenderResult): seq[int] =
    for g in r.regions: result.add g.line

  test "JSON : chaque valeur renvoie à sa ligne":
    let r = renderPuml("@startjson\n{\n  \"a\": 1,\n  \"b\": {\n    \"c\": true\n  },\n  \"d\": [1,\n    2]\n}\n@endjson\n")
    check r.errors.len == 0
    let l = r.lines
    check 3 in l
    check 5 in l
    check 8 in l

  test "JSON invalide : ligne de l'erreur":
    let r = renderPuml("@startjson\n{\n  \"a\": 1,\n  oups\n}\n@endjson\n")
    check r.errors.len == 1
    check r.diags[0].line == 4

  test "YAML : chaque clé renvoie à sa ligne":
    let r = renderPuml("@startyaml\na: 1\nb:\n  c: 2\n  d:\n    - x\n    - y: 3\n@endyaml\n")
    check r.errors.len == 0
    let l = r.lines
    for n in [2, 3, 4, 5, 6, 7]: check n in l

  test "YAML : listes et objets en ligne":
    let r = renderPuml("@startyaml\na: [1, [2, 3], {k: v}]\n@endyaml\n")
    check r.errors.len == 0
    check "k" in r.svg and "v" in r.svg

suite "rendu (revue visuelle)":
  proc numbersAfter(svg, tag: string): seq[float] =
    ## Coordonnées d'un premier attribut « tag="…" » de chaque élément.
    var i = 0
    while true:
      i = svg.find(tag & "=\"", i)
      if i < 0: break
      i += tag.len + 2
      result.add parseFloat(svg[i ..< svg.find('"', i)])

  test "auto-message : le cadre englobe son libellé":
    let label = "un libellé d'auto-message nettement plus long que les colonnes"
    let r = renderPuml("@startuml\nA -> B\nalt x\nA -> A : " & label & "\nelse y\nA -> B\nend\n@enduml\n")
    check r.errors.len == 0
    let width = textWidth(label, 12)
    # le cadre le plus à droite dépasse la fin du libellé (colonne A + 8 + largeur)
    let xs = r.svg.numbersAfter("width")
    check xs.anyIt(it >= width)
    check r.width >= width

  test "arêtes parallèles entre boîtes : traits séparés":
    let r = renderPuml("@startuml\nstate A\nstate B\nA --> B : aller\nB --> A : retour\n@enduml\n")
    # deux traits droits (une seule commande L), restés écartés jusqu'aux bords
    var xs: seq[float]
    var i = 0
    while true:
      i = r.svg.find("d=\"M", i)
      if i < 0: break
      i += 4
      let d = r.svg[i ..< r.svg.find('"', i)]
      if d.count(" L") == 1: xs.add parseFloat(d.split(' ')[0])
    check xs.len == 2
    check abs(xs[0] - xs[1]) > 15

  test "libellé d'arête jamais hors du cadre":
    let r = renderPuml("@startuml\nstate A\nstate B\nA --> B : un libellé plutôt long à droite\nB --> A : un autre libellé long à gauche\n@enduml\n")
    for g in r.regions: check g.x >= 0 and g.y >= 0
    for g in r.regions: check g.x + g.w <= r.width

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
