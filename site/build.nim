## Génère le site statique à partir de `docs/*.md`, sans dépendance.
## nim c -r site/build.nim  ->  site/dist/
import std/[os, sequtils, strutils, tables, unicode]

const
  Depot = "https://git.sethmartin.dev/Seth_Martin/simp2puml"
  Branche = "main"
  Source = Depot & "/src/branch/" & Branche & "/"
  Url = "https://simp2puml.sethmartin.dev"

type
  Page = object
    slug, titre, groupe, resume: string
  Titre = object
    niveau: int
    texte, id: string
  Entree = object
    page, titre, url, texte: string

const Pages = [
  Page(slug: "installation", titre: "Installation", groupe: "Démarrer",
       resume: "Construire l'éditeur, ou installer le convertisseur sans rien connaître de Python."),
  Page(slug: "editeur", titre: "L'éditeur", groupe: "L'éditeur",
       resume: "Écriture, clics dans le diagramme, préférences, profils de sécurité et raccourcis."),
  Page(slug: "moteur", titre: "Moteur de rendu natif", groupe: "L'éditeur",
       resume: "Les diagrammes couverts sans Java, l'usage du moteur seul et les performances mesurées."),
  Page(slug: "conversion", titre: "Ligne de commande", groupe: "Le convertisseur",
       resume: "Convertir et vérifier entre SIMP, PlantUML et Atelier Diagrammes."),
  Page(slug: "couverture", titre: "Couverture et limites", groupe: "Le convertisseur",
       resume: "Ce que devient chaque famille SIMP, et ce qui se perd."),
  Page(slug: "atelier", titre: "Export Atelier", groupe: "Le convertisseur",
       resume: "Le DSL 2.0 d'Atelier Diagrammes pour Typst."),
  Page(slug: "interface-classique", titre: "Interface classique", groupe: "Le convertisseur",
       resume: "L'interface Tkinter, sans dépendance."),
  Page(slug: "developpement", titre: "Développement", groupe: "Contribuer",
       resume: "Organisation du code, tests et paquets natifs."),
]

proc echapper(s: string): string =
  s.multiReplace(("&", "&amp;"), ("<", "&lt;"), (">", "&gt;"), ("\"", "&quot;"))

proc identifiant(texte: string): string =
  ## Identifiant d'ancre : minuscules sans accents.
  const Accents = {"à": "a", "â": "a", "ç": "c", "é": "e", "è": "e", "ê": "e", "ë": "e",
                   "î": "i", "ï": "i", "ô": "o", "ù": "u", "û": "u", "œ": "oe"}.toTable
  for rune in unicode.toLower(texte).runes:
    let c = $rune
    let s = Accents.getOrDefault(c, c)
    if s.len == 1 and s[0] in {'a'..'z', '0'..'9'}: result.add s
    elif s.len > 1 and s[0] in {'a'..'z'}: result.add s
    elif result.len > 0 and result[^1] != '-': result.add '-'
  result = result.strip(chars = {'-'})

proc lien(cible: string; versRacine: string): string =
  ## Réécrit un lien de `docs/` pour le site ; un fichier du dépôt renvoie à Forgejo.
  if cible.startsWith("http") or cible.startsWith("#") or cible.startsWith("mailto:"): return cible
  if cible.startsWith("images/"): return versRacine & cible
  let morceaux = cible.split('#', 1)
  if morceaux[0].endsWith(".md") and "/" notin morceaux[0]:
    result = versRacine & "guide/" & morceaux[0][0 .. ^4] & "/"
    if morceaux.len > 1: result.add "#" & morceaux[1]
    return
  Source & cible.replace("../", "")

proc enLigne(s: string; versRacine: string): string =
  ## Code, gras, italique et liens ; tout le reste est échappé.
  var i = 0
  while i < s.len:
    let c = s[i]
    if c == '`':
      var n = 0
      while i + n < s.len and s[i + n] == '`': inc n
      let fin = s.find(repeat('`', n), i + n)
      if fin >= 0:
        result.add "<code>" & echapper(s[i + n ..< fin].strip) & "</code>"
        i = fin + n
        continue
    if c == '[':
      let fin = s.find("](", i)
      if fin > 0:
        let ferme = s.find(')', fin)
        if ferme > 0:
          result.add "<a href=\"" & echapper(lien(s[fin + 2 ..< ferme], versRacine)) & "\">" &
            enLigne(s[i + 1 ..< fin], versRacine) & "</a>"
          i = ferme + 1
          continue
    if s.continuesWith("**", i):
      let fin = s.find("**", i + 2)
      if fin > 0:
        result.add "<strong>" & enLigne(s[i + 2 ..< fin], versRacine) & "</strong>"
        i = fin + 2
        continue
    if c == '*' and i + 1 < s.len and s[i + 1] != ' ':
      let fin = s.find('*', i + 1)
      if fin > 0 and s[fin - 1] != ' ':
        result.add "<em>" & enLigne(s[i + 1 ..< fin], versRacine) & "</em>"
        i = fin + 1
        continue
    result.add echapper($c)
    inc i

proc texteBrut(s: string): string =
  ## Le texte lisible d'une ligne, pour la recherche et les ancres.
  var i = 0
  while i < s.len:
    if s[i] == '[':
      let fin = s.find("](", i)
      let ferme = if fin > 0: s.find(')', fin) else: -1
      if ferme > 0:
        result.add s[i + 1 ..< fin]
        i = ferme + 1
        continue
    if s[i] notin {'`', '*'}: result.add s[i]
    inc i

proc image(alt, src, versRacine: string; classe = "capture"): string =
  ## Un GIF cède la place à une image fixe si le lecteur préfère moins de mouvement.
  let chemin = versRacine & src
  let (_, nom, ext) = splitFile(src)
  let dimensions = if ext == ".gif" or classe == "capture": " width=\"1200\" height=\"750\"" else: ""
  if ext == ".gif":
    result = "<picture><source media=\"(prefers-reduced-motion: reduce)\" srcset=\"" & versRacine &
      "images/fixes/" & nom & ".png\"><img src=\"" & chemin & "\" alt=\"" & echapper(alt) & "\" loading=\"lazy\"" &
      dimensions & "></picture>"
  else:
    result = "<img src=\"" & chemin & "\" alt=\"" & echapper(alt) & "\" loading=\"lazy\">"

proc rendre(md: string; versRacine: string; titres: var seq[Titre]; recherche: var seq[Entree]; page: Page): string =
  let lignes = md.splitLines()
  var i = 0
  var o: seq[string]
  var vus = initCountTable[string]()
  let urlPage = "guide/" & page.slug & "/"
  var courant = Entree(page: page.titre, titre: page.titre, url: urlPage)
  template pousser() =
    if courant.texte.strip.len > 0 or courant.titre != page.titre:
      recherche.add courant
  while i < lignes.len:
    let l = lignes[i]
    if l.strip.len == 0:
      inc i
    elif l.startsWith("```"):
      let langue = l[3 .. ^1].strip
      inc i
      var code: seq[string]
      while i < lignes.len and not lignes[i].startsWith("```"):
        code.add lignes[i]
        inc i
      inc i
      let classe = if langue.len > 0: " class=\"language-" & echapper(langue) & "\"" else: ""
      o.add "<pre><code" & classe & ">" & echapper(code.join("\n")) & "</code></pre>"
      courant.texte.add " " & code.join(" ")
    elif l.startsWith("#"):
      var n = 0
      while n < l.len and l[n] == '#': inc n
      let brut = l[n .. ^1].strip
      if n == 1:
        inc i
        continue # le titre de page vient du gabarit
      var id = identifiant(texteBrut(brut))
      vus.inc id
      if vus[id] > 1: id.add "-" & $vus[id]
      titres.add Titre(niveau: n, texte: texteBrut(brut), id: id)
      o.add "<h" & $n & " id=\"" & id & "\"><a class=\"ancre\" href=\"#" & id & "\" aria-label=\"Lien vers cette section\">#</a>" &
        enLigne(brut, versRacine) & "</h" & $n & ">"
      pousser()
      courant = Entree(page: page.titre, titre: texteBrut(brut), url: urlPage & "#" & id)
      inc i
    elif l.startsWith("<a id="):
      o.add l
      inc i
    elif l.startsWith("!["):
      let fin = l.find("](")
      let alt = l[2 ..< fin]
      let src = l[fin + 2 ..< l.rfind(')')]
      o.add "<figure class=\"capture\">" & image(alt, src, versRacine) & "<figcaption>" & echapper(alt) & "</figcaption></figure>"
      inc i
    elif l.startsWith("|"):
      var rangs: seq[seq[string]]
      while i < lignes.len and lignes[i].startsWith("|"):
        let cellules = lignes[i].strip(chars = {' ', '|'}).split(" | ").mapIt(it.strip)
        rangs.add cellules
        inc i
      var t = "<div class=\"tableau\"><table><thead><tr>"
      for c in rangs[0]: t.add "<th>" & enLigne(c, versRacine) & "</th>"
      t.add "</tr></thead><tbody>"
      for rang in rangs[2 .. ^1]:
        t.add "<tr>"
        for c in rang:
          t.add "<td>" & enLigne(c, versRacine) & "</td>"
          courant.texte.add " " & texteBrut(c)
        t.add "</tr>"
      t.add "</tbody></table></div>"
      o.add t
    elif l.startsWith("- "):
      var t = "<ul>"
      while i < lignes.len and lignes[i].startsWith("- "):
        t.add "<li>" & enLigne(lignes[i][2 .. ^1], versRacine) & "</li>"
        courant.texte.add " " & texteBrut(lignes[i][2 .. ^1])
        inc i
      o.add t & "</ul>"
    else:
      var p = l
      inc i
      while i < lignes.len and lignes[i].strip.len > 0 and lignes[i][0] notin {'#', '|', '`', '!', '<'} and not lignes[i].startsWith("- "):
        p.add " " & lignes[i]
        inc i
      o.add "<p>" & enLigne(p, versRacine) & "</p>"
      courant.texte.add " " & texteBrut(p)
  pousser()
  o.join("\n")

proc jsonTexte(s: string): string =
  result = "\""
  for rune in s.runes:
    case $rune
    of "\"": result.add "\\\""
    of "\\": result.add "\\\\"
    of "\n", "\r", "\t": result.add " "
    else:
      if rune.int32 >= 32: result.add $rune
  result.add "\""

proc navigation(actif: string; versRacine: string): string =
  var groupe = ""
  for p in Pages:
    if p.groupe != groupe:
      if groupe.len > 0: result.add "</ul>"
      groupe = p.groupe
      result.add "<p class=\"nav-groupe\">" & groupe & "</p><ul>"
    let courant = if p.slug == actif: " aria-current=\"page\"" else: ""
    result.add "<li><a href=\"" & versRacine & "guide/" & p.slug & "/\"" & courant & ">" & echapper(p.titre) & "</a></li>"
  result.add "</ul>"

proc gabarit(squelette: string; valeurs: openArray[(string, string)]): string =
  result = squelette
  for (cle, valeur) in valeurs:
    result = result.replace("{{" & cle & "}}", valeur)

let ici = currentSourcePath().parentDir
let racine = ici.parentDir
let sortie = ici / "dist"
removeDir(sortie)
createDir(sortie / "guide")

let enveloppe = readFile(ici / "templates" / "page.html")
var recherche: seq[Entree]

for index, p in Pages:
  var titres: seq[Titre]
  let corps = rendre(readFile(racine / "docs" / (p.slug & ".md")), "../../", titres, recherche, p)
  var sommaire = ""
  if titres.len > 1:
    sommaire = "<nav class=\"sommaire\" aria-label=\"Sur cette page\"><p>Sur cette page</p><ul>"
    for t in titres:
      if t.niveau <= 3:
        sommaire.add "<li class=\"n" & $t.niveau & "\"><a href=\"#" & t.id & "\">" & echapper(t.texte) & "</a></li>"
    sommaire.add "</ul></nav>"
  var suite = ""
  if index > 0:
    suite.add "<a class=\"precedent\" href=\"../" & Pages[index - 1].slug & "/\"><span>Précédent</span>" & echapper(Pages[index - 1].titre) & "</a>"
  if index < Pages.high:
    suite.add "<a class=\"suivant\" href=\"../" & Pages[index + 1].slug & "/\"><span>Suivant</span>" & echapper(Pages[index + 1].titre) & "</a>"
  let html = gabarit(enveloppe, {
    "titre": echapper(p.titre), "resume": echapper(p.resume), "racine": "../../", "url": Url & "/guide/" & p.slug & "/",
    "navigation": navigation(p.slug, "../../"), "sommaire": sommaire, "corps": corps, "suite": suite,
    "groupe": echapper(p.groupe), "source": Source & "docs/" & p.slug & ".md",
  })
  createDir(sortie / "guide" / p.slug)
  writeFile(sortie / "guide" / p.slug / "index.html", html)

writeFile(sortie / "index.html", readFile(ici / "templates" / "accueil.html"))
writeFile(sortie / "404.html", readFile(ici / "templates" / "404.html"))

var json = "["
for i, e in recherche:
  if i > 0: json.add ","
  json.add "{\"p\":" & jsonTexte(e.page) & ",\"t\":" & jsonTexte(e.titre) & ",\"u\":" & jsonTexte(e.url) & ",\"x\":" & jsonTexte(e.texte.strip) & "}"
writeFile(sortie / "recherche.json", json & "]")

copyDir(racine / "docs" / "images", sortie / "images")
copyFile(racine / "src" / "simp2puml" / "assets" / "portrait-89.png", sortie / "portrait.png")
copyFile(racine / "src" / "simp2puml" / "assets" / "icon-64.png", sortie / "favicon.png")
for fichier in ["style.css", "site.js"]:
  copyFile(ici / "static" / fichier, sortie / fichier)
echo "site généré : ", Pages.len, " pages, ", recherche.len, " sections indexées"
