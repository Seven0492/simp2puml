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
  test "aucun fichier n'est jamais lu par le moteur":
    let r = renderPuml("@startuml\n!include /etc/passwd\n!includeurl http://x.example/a\n$x = %getenv(\"HOME\")\nclass A\n@enduml\n")
    check r.errors.len == 0
    check r.warnings.anyIt("!include" in it)
    check "root:" notin r.svg
    check getEnv("HOME") notin r.svg or getEnv("HOME").len == 0

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
    let r = renderPuml("@startebnf\na = b;\n@endebnf\n")
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

suite "types et blocs (évaluation)":
  test "légende et en-têtes en deux mots":
    let r = renderPuml("@startuml\nheader\nH\nend header\nlegend right\nL\nend legend\nclass A\n@enduml\n")
    check r.kind == dkClass
    check r.warnings.len == 0
    check "L" in r.svg

  test "entité avec un corps : diagramme de classes":
    let r = renderPuml("@startuml\nentity E {\n*id : int\n--\nn : text\n}\n@enduml\n")
    check r.kind == dkClass
    check r.warnings.len == 0
    check "id : int" in r.svg

suite "préprocesseur sûr":
  proc body(src: string): RenderResult = renderPuml("@startuml\n" & src & "\n@enduml\n")

  test "!define avec et sans paramètres":
    let r = body("!define NOM Livre\n!define BOITE(n) class n #lightblue\nBOITE(NOM)")
    check r.errors.len == 0 and r.warnings.len == 0
    check "Livre" in r.svg and "lightblue" in r.svg

  test "!definelong et !undef":
    let r = body("!definelong DEUX(a)\nclass a\nclass Autre\n!enddefinelong\nDEUX(Z)\n!define X Y\n!undef X\nclass X")
    check r.errors.len == 0
    check "Z" in r.svg and "Autre" in r.svg and ">X<" in r.svg

  test "variables, conditions, boucles, procédures et fonctions":
    let r = body("""!$t = "Titre" + " final"
!function $d($x)
!return $x * 2
!endfunction
!procedure $c($n)
class $n
!endprocedure
title $t $d(21)
!if ($d(2) == 4)
$c(Oui)
!else
$c(Non)
!endif
!$i = 0
!while $i < 2
class C$i
!$i = $i + 1
!endwhile
!ifdef ABSENT
class Jamais
!endif""")
    check r.errors.len == 0 and r.warnings.len == 0
    check "Titre final 42" in r.svg
    check "Oui" in r.svg and "Non" notin r.svg and "Jamais" notin r.svg
    check "C0" in r.svg and "C1" in r.svg

  test "les lignes restent celles du texte":
    let r = body("!define A class B\n\nce n'est pas du plantuml ???")
    check r.warnings.len == 1
    check r.diags[0].line == 4

  test "boucles et récursions bornées":
    let infinite = body("!$i = 0\n!while 1\n!$i = $i + 1\n!endwhile\nclass A")
    check infinite.errors.anyIt("tours" in it)
    check validSvg(infinite.svg)
    let self = body("!function $f($x)\n!return $f($x)\n!endfunction\nclass $f(1)")
    check self.errors.anyIt("trop profonds" in it)
    check validSvg(self.svg)
    check validSvg(body("!define A A A\nA").svg)
    check validSvg(body("!define A B B\n!define B A A\nA").svg)

  test "fonctions à accès extérieur refusées":
    for f in ["%getenv(\"HOME\")", "%load_json(\"/etc/passwd\")", "%file_exists(\"/etc/passwd\")"]:
      let r = body("!$x = " & f & "\nclass A")
      check r.warnings.anyIt("refusée" in it)

  test "JSON et YAML gardent « ! » et « $ » tels quels":
    check "$prix" in renderPuml("@startjson\n{\"a\": \"$prix\"}\n@endjson\n").svg

suite "styles":
  proc body(src: string): RenderResult = renderPuml("@startuml\n" & src & "\n@enduml\n")

  test "skinparam par élément, en bloc et global":
    let r = body("skinparam backgroundColor #FEFECE\nskinparam class {\nBackgroundColor #FFD700\nBorderColor #8B0000\n}\nskinparam ArrowColor #008000\nskinparam noteBackgroundColor #E0FFFF\nclass A\nA --> B\nnote right of B : n")
    check r.errors.len == 0
    for c in ["#FEFECE", "#FFD700", "#8B0000", "#008000", "#E0FFFF"]: check c in r.svg

  test "skinparam n'atteint pas les autres éléments ni le rendu suivant":
    check "#FFD700" notin body("skinparam componentBackgroundColor #FFD700\nclass A").svg
    discard body("skinparam backgroundColor #123456\nclass A")
    check "#123456" notin body("class A").svg

  test "couleur et taille par défaut":
    let r = body("skinparam defaultFontColor #112233\nskinparam defaultFontSize 20\nclass A")
    check "#112233" in r.svg
    check "font-size=\"20\"" in r.svg

  test "nom de police sûr":
    let r = body("skinparam defaultFontName \"x\\\" onload=\\\"y\"\nclass A")
    check validSvg(r.svg)
    check "onload" notin r.svg

  test "<style> imbriqué":
    let r = body("<style>\ndocument { BackGroundColor #EEEEFF }\nclassDiagram {\nclass { BackGroundColor #FFCCCC\nLineColor #CC0000 }\narrow { LineColor #0000CC }\n}\n</style>\nclass A\nA --> B")
    for c in ["#EEEEFF", "#FFCCCC", "#CC0000", "#0000CC"]: check c in r.svg

  test "!theme":
    check body("!theme hacker\nclass A").svg != body("class A").svg
    check body("!theme inconnu\nclass A").warnings.anyIt("thème inconnu" in it)
    check body("!theme x from http://mal.example\nclass A").warnings.anyIt("refusé" in it)

  test "créole : morceaux séparés et largeur sans le balisage":
    let r = body("class A {\n**gras** //it// <color:red>rouge</color>\n}")
    check "font-weight=\"bold\">gras<" in r.svg
    check "font-style=\"italic\">it<" in r.svg
    check "fill=\"red\"" in r.svg
    check "**" notin r.svg and "<color" notin r.svg
    check textWidth("**ab**") == textWidth("ab", bold = true)
    check "http://x.example" in body("class A {\nhttp://x.example\n}").svg

suite "types ajoutés":
  test "salt":
    let r = renderPuml("@startsalt\n{+\nNom | \"****\"\n[X] Oui | () Non\n^Liste^ | [Valider]\n--\n{T\n+ Racine\n++ Enfant\n}\n}\n@endsalt\n")
    check r.kind == dkSalt
    check r.errors.len == 0 and r.warnings.len == 0
    check validSvg(r.svg)
    for s in ["Nom", "****", "Oui", "Non", "Liste", "Valider", "Racine", "Enfant"]: check s in r.svg
    check renderPuml("@startuml\nsalt\n{\nA | B\n}\n@enduml\n").kind == dkSalt

  test "salt : grille non fermée et imbrication extrême":
    check renderPuml("@startsalt\n{\nA\n@endsalt\n").errors.len == 1
    var deep = "@startsalt\n"
    for _ in 1 .. 5_000: deep.add "{\n"
    check renderPuml(deep & "@endsalt\n").errors.anyIt("imbrication" in it)

  test "timing":
    let r = renderPuml("@startuml\nrobust \"Nav\" as N\nconcise \"Util\" as U\nclock \"H\" as C with period 10\nbinary \"B\" as B\n@0\nN is Repos\nU is Lire\nB is low\n@50\nN is Actif\nB is high\n@+20 as :fin\nU is Fin\nU -> N : clic\n@N\n60 is Repos\nhighlight 10 to 20 #Gold : zone\n@enduml\n")
    check r.kind == dkTiming
    check r.errors.len == 0 and r.warnings.len == 0
    check validSvg(r.svg)
    for s in ["Nav", "Util", "Repos", "Actif", "Lire", "clic", "zone"]: check s in r.svg

  test "timing : période minuscule bornée":
    let r = renderPuml("@startuml\nclock c with period 0.000001\nbinary b\n@0\nb is low\n@1000000\nb is high\n@enduml\n")
    check validSvg(r.svg)
    check r.width < 300_000

  test "ditaa":
    let r = renderPuml("@startditaa\n+-----+  +---+\n|cBLU | -->| B |\n| Txt |  +---+\n+-----+\n@endditaa\n")
    check r.kind == dkDitaa
    check validSvg(r.svg)
    check "Txt" in r.svg and "#3366FF" in r.svg and "cBLU" notin r.svg

  test "archimate":
    let r = renderPuml("@startuml\narchimate #Technology \"Serveur\" as s <<technology-device>>\narchimate #Application \"Appli\" as a\na --> s\n@enduml\n")
    check r.errors.len == 0 and r.warnings.len == 0
    check "Serveur" in r.svg and "#C9FFC9" in r.svg and "#C2F0FF" in r.svg

  test "sprite : dans le texte et en stéréotype":
    let sprite = "sprite $s [2x2/16] {\nFF\n0F\n}\n"
    let r = renderPuml("@startuml\n" & sprite & "class A <<$s>>\nclass \"Avec <$s>\" as B\n@enduml\n")
    check r.errors.len == 0 and r.warnings.len == 0
    check "fill-opacity" in r.svg
    check "<$s>" notin r.svg and "$s" notin r.svg
    check renderPuml("@startuml\nsprite $z [4x4/16z] ABC\nclass A\n@enduml\n").warnings.anyIt("compressé" in it)
    check renderPuml("@startuml\nsprite $z {\nGG\n}\nclass A\n@enduml\n").warnings.anyIt("illisible" in it)

  test "« & » : même hauteur que le message précédent":
    let r = renderPuml("@startuml\nA -> B : un\n& A -> C : deux\nB -> A : trois\n@enduml\n")
    check r.warnings.len == 0
    proc baseline(text: string): string =
      let at = r.svg.find(">" & text & "<")
      let tag = r.svg.rfind("<text", 0, at)
      let start = r.svg.find(" y=\"", tag) + 4
      r.svg[start ..< r.svg.find('"', start)]
    check baseline("un") == baseline("deux")
    check baseline("trois") != baseline("un")

suite "clics précis":
  proc at(r: RenderResult, x, y: float): int =
    ## Ligne de la plus petite région sous le point, comme l'éditeur.
    var best = Inf
    for g in r.regions:
      if x >= g.x and x <= g.x + g.w and y >= g.y and y <= g.y + g.h and g.w * g.h < best:
        best = g.w * g.h
        result = g.line

  proc smallest(r: RenderResult, line: int): Region =
    ## Une zone de cette ligne, la plus petite.
    var area = Inf
    for g in r.regions:
      if g.line == line and g.w * g.h < area:
        area = g.w * g.h
        result = g

  proc hit(r: RenderResult, line: int): int =
    let g = r.smallest(line)
    r.at(g.x + g.w / 2, g.y + g.h / 2)

  test "chaque membre d'une classe renvoie à sa ligne":
    let r = renderPuml("""@startuml
class A {
  +x : int
  -y : String
  --
  +f() : void
  .. titre ..
  +g()
}
A : +z : int
enum E {
  UN
  DEUX
}
@enduml
""")
    check r.errors.len == 0
    for line in [3, 4, 6, 7, 8, 10, 12, 13]: check r.hit(line) == line
    # l'en-tête reste la déclaration
    check r.smallest(2).h > 20

  test "le même texte deux fois garde ses deux lignes":
    let r = renderPuml("@startuml\nclass A {\n  x\n  x\n}\n@enduml\n")
    check r.hit(3) == 3
    check r.hit(4) == 4

  test "un objet, une carte et un état":
    let o = renderPuml("@startuml\nobject O {\n  a = 1\n  b = 2\n}\nmap M {\n  k => v\n}\n@enduml\n")
    for line in [3, 4, 7]: check o.hit(line) == line
    let s = renderPuml("@startuml\nstate S\nS : première\nS : seconde\n@enduml\n")
    for line in [3, 4]: check s.hit(line) == line

  test "une relation donne son tracé, pas des zones carrées":
    let r = renderPuml("@startuml\nclass A\nclass B\nA \"1\" --> \"*\" B : lien\n@enduml\n")
    check r.links.len == 1
    check r.links[0].line == 4
    check r.links[0].points.len >= 2
    check r.regions.countIt(it.line == 4 and it.w == 10 and it.h == 10) == 0
    # du bord d'une boîte à l'autre : vertical, entre les deux classes
    let (p0, p1) = (r.links[0].points[0], r.links[0].points[^1])
    check p1[1] - p0[1] > 30
    # le libellé et les multiplicités restent cliquables
    check r.hit(4) == 4

  test "le tracé suit le décalage de l'habillage":
    let plain = renderPuml("@startuml\nA --|> B\n@enduml\n")
    let titled = renderPuml("@startuml\ntitle Un titre\nA --|> B\n@enduml\n")
    check titled.links[0].points[0][1] > plain.links[0].points[0][1]

  test "un libellé qui décale tout le diagramme décale aussi le tracé":
    let r = renderPuml("@startuml\nstate A\nstate B\nA --> B : un libellé plutôt long à droite\nB --> A : un autre libellé long à gauche\n@enduml\n")
    check r.links.len == 2
    for l in r.links:
      for p in l.points: check p[0] >= 0 and p[0] <= r.width

  test "autres flèches : séquence, boucle sur soi, chronogramme, activité, données, carte":
    check renderPuml("@startuml\nA -> B : m\nA -> A : soi\n@enduml\n").links.len == 2
    check renderPuml("@startuml\nclass A\nA --> A\n@enduml\n").links.len == 1
    check renderPuml("@startuml\nrobust a\nrobust b\n@0\na is X\nb is Y\na -> b : m\n@enduml\n").links.len == 1
    check renderPuml("@startuml\nstart\n:a;\n:b;\nstop\n@enduml\n").links.len == 3
    check renderPuml("@startjson\n{\"a\": {\"b\": 1}}\n@endjson\n").links.len == 1
    check renderPuml("@startmindmap\n* r\n** a\n** b\n@endmindmap\n").links.len == 2
    check renderPuml("@startwbs\n* r\n** a\n@endwbs\n").links.len == 1

  test "les éléments dessinés après la flèche passent au-dessus":
    let r = renderPuml("@startuml\nclass A\nclass B\nA --> B\n@enduml\n")
    check r.links[0].after < r.regions.len
    check r.regions[r.links[0].after ..< r.regions.len].anyIt(it.h > 20)

  test "beaucoup de relations : un tracé par relation, sans zones en plus":
    var src = "@startuml\n"
    for i in 0 .. 400: src.add "A" & $i & " --> A" & $(i + 1) & "\n"
    let r = renderPuml(src & "@enduml\n")
    check r.links.len == 401
    check r.regions.len < 1000

  test "séquence : activation, else et note":
    let r = renderPuml("""@startuml
A -> B
activate B
alt oui
A -> B : m
else non
B -> A : r
end
note over A : n
deactivate B
@enduml
""")
    for line in [2, 3, 4, 5, 6, 7, 9]: check r.hit(line) == line

  test "activité : branches, note, flèche et bas de boucle":
    let r = renderPuml("""@startuml
start
if (c ?) then (oui)
  :a;
  -> étiquette;
  :b;
  note right : n
else (non)
  :d;
endif
repeat
  :e;
repeat while (encore ?) is (oui)
stop
@enduml
""")
    check r.errors.len == 0
    for line in [3, 4, 5, 6, 7, 8, 9, 12, 13]: check r.hit(line) == line

  test "titre, en-tête, légende de figure et pied de page":
    let r = renderPuml("@startuml\nheader H\nfooter F\ncaption C\nclass A\n@enduml\n")
    for line in [2, 3, 4]: check r.hit(line) == line

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
