# simp2puml

Outil en ligne de commande et interface graphique pour convertir des diagrammes SIMP vers PlantUML, et expérimentalement de PlantUML vers SIMP. Les deux formats s’exportent aussi vers le DSL 2.0 d’[Atelier Diagrammes](https://git.sethmartin.dev/Seth_Martin/atelier-diagrammes) (blocs Typst `diagramme-ad`). Python ≥ 3.11 ; aucune dépendance tierce à l’exécution. Le modèle commun conserve les identités, classifieurs, membres, relations, notes et avertissements, sans reproduire fidèlement toutes les données du format source.

## Origine et crédits

Ce projet reprend le convertisseur créé par **ASaki**, disponible dans le [dépôt GitHub d’origine de Nela](https://github.com/2534296-Mailloux-Nela/simp2puml). Merci à ASaki pour le travail initial sur la conversion SIMP et PlantUML, la CLI et le packaging.

Cette version ajoute des corrections de fidélité, des options de vérification et des diagnostics plus précis. La [licence MIT](LICENSE) et la mention de copyright d’origine de Nela sont conservées.

## Démarrer en un double-clic (sans rien installer)

Pour utiliser l’interface graphique sans connaître Python, télécharger le projet (archive ZIP), l’extraire, puis ouvrir le lanceur de son système :

| Système | Lanceur | Premier lancement |
| --- | --- | --- |
| Windows | `simp2puml-windows.cmd` | Double-clic. Si Windows affiche un avertissement (fichier téléchargé), choisir « Exécuter ». Raccourcis « simp2puml » créés dans le menu Démarrer et sur le Bureau. |
| macOS | `simp2puml-macos.command` | Clic droit → Ouvrir la première fois (Gatekeeper bloque le double-clic sur un script téléchargé). L’application « simp2puml » est ensuite ajoutée à `~/Applications` (Launchpad, Spotlight). |
| Linux | `simp2puml-linux.sh` | « Exécuter » depuis le gestionnaire de fichiers, ou `sh simp2puml-linux.sh`. Une entrée « simp2puml » est ajoutée au menu des applications. |

Le lanceur utilise un Python ≥ 3.11 avec Tkinter s’il en trouve un. Sinon, il télécharge [uv](https://docs.astral.sh/uv/) (version fixée ; son installateur vérifie les sommes SHA-256), qui installe un Python 3.12 privé, **sans droits administrateur et sans toucher au Python du système**. Tout est rangé dans un seul dossier : `%LOCALAPPDATA%\simp2puml` (Windows), `~/Library/Application Support/simp2puml` (macOS) ou `~/.local/share/simp2puml` (Linux). Désinstaller revient à supprimer ce dossier et le raccourci créé. Une connexion Internet n’est nécessaire qu’au premier lancement. Les raccourcis pointent vers le dossier extrait : relancer le lanceur après l’avoir déplacé.

Le lanceur Windows a été vérifié sur Windows 11, avec et sans Python préinstallé. Les lanceurs macOS et Linux sont vérifiés par ShellCheck, mais n’ont pas encore été exécutés sur ces systèmes.

## Installer et convertir

La méthode recommandée depuis un clone est `pipx`, qui isole l’outil de Python système :

```sh
# Installer pipx avec le gestionnaire de paquets de votre système, si nécessaire :
# Debian/Ubuntu : sudo apt install pipx
# Fedora :        sudo dnf install pipx
pipx install .
pipx ensurepath
```

Ouvrez une nouvelle session de terminal après `pipx ensurepath` si la commande n’est pas trouvée. Pour installer une wheel construite localement :

```sh
pipx install dist/simp2puml-0.1.0-py3-none-any.whl
```

La wheel inclut aussi la page de manuel. Sur Linux, avec `man` installé, pipx l’expose et elle se consulte après installation avec `man simp2puml`. Si une ancienne wheel était déjà installée, la remplacer par la nouvelle avec `pipx install --force dist/simp2puml-0.1.0-py3-none-any.whl`.

Si `man` ne trouve pas la page, vérifier le dossier `PIPX_MAN_DIR` indiqué par `pipx environment` et sa présence dans les chemins de recherche de man. Avec le chemin pipx par défaut, `man -M ~/.local/share/man simp2puml` permet aussi de la consulter. L’aide `simp2puml --help` reste disponible sans programme `man`.

La version 0.1.0 n’est pas annoncée comme publiée sur PyPI : `pipx install simp2puml` n’est pas la procédure d’installation. Alternative isolée sans pipx :

```sh
python3 -m venv .venv
.venv/bin/python -m pip install .
.venv/bin/simp2puml --version
```

Ne pas installer avec `sudo pip` ni contourner la protection Python système avec `--break-system-packages`.

```sh
simp2puml --version
simp2puml diagramme.simp
simp2puml diagramme.simp -o resultat.puml
simp2puml projet.simp --diagram "Nom du diagramme" -o resultat.puml
simp2puml projet.simp --list-diagrams
simp2puml projet.simp --all -o projet.puml
simp2puml diagramme.simp --check
simp2puml diagramme.simp --strict -o resultat.puml
simp2puml diagramme.simp --to atelier
simp2puml diagramme.puml -o schema.atelier
```

L’extension source choisit le sens de conversion ; sans `-o` ni `--to`, le fichier de destination prend l’extension opposée dans le même dossier. `--to puml|simp|atelier` choisit explicitement le format cible ; sans `--to`, une sortie `-o` en `.atelier` le sélectionne aussi. Le format cible doit différer du format source. `--diagram` sélectionne un diagramme SIMP de tout type par nom ou identifiant. Sans sélection, un projet comportant plusieurs diagrammes est refusé. `--list-diagrams` affiche une ligne JSON par diagramme (identifiant, nom et type), sans conversion. `--all` produit un fichier `.puml` contenant un bloc `@startuml` distinct par vue, dans l’ordre du projet ; les diagrammes ne sont pas fusionnés. Toutes les conversions doivent réussir avant publication du fichier. Une sortie existante ou identique à la source est refusée. La source est limitée à 20 MiB, lue en UTF-8 (BOM accepté) ; la sortie est publiée atomiquement.

`--check` effectue la lecture et la génération du format cible en mémoire, sans créer ni modifier de fichier. Il permet de vérifier la conversion même si la sortie par défaut existe déjà ; il ne s’utilise pas avec `-o`. `--strict` refuse toute conversion accompagnée d’avertissements, avant d’écrire la sortie. Les deux options se combinent pour une validation automatisée :

```sh
simp2puml projet.simp --diagram "Nom du diagramme" --check --strict
```

Le mode strict refuse aussi toute génération SIMP, car celle-ci émet toujours un avertissement expérimental. Il ne prouve pas l’absence de toute perte : seuls les avertissements détectés font échouer la commande. Codes de sortie : `0` pour une conversion ou vérification réussie, `1` pour une erreur de conversion, d’entrée/sortie ou un refus strict, `2` pour des arguments invalides.

## Interface graphique

L’installation fournit aussi `simp2puml-gui` (ou `python -m simp2puml.gui [fichier]`), une fenêtre Tkinter sans dépendance supplémentaire. Elle exige un Python livré avec Tk : c’est le cas des installateurs Windows et macOS de python.org ; sous Linux, installer le paquet de la distribution (`python3-tk` sous Debian/Ubuntu, `python3-tkinter` sous Fedora).

- **Ouvrir…** (Ctrl+O) choisit une source `.simp` ou `.puml` ; le format cible opposé est présélectionné. Pour un projet SIMP, la liste propose chaque vue (nom et type) et, s’il y en a plusieurs, l’équivalent de `--all`, qui force la sortie PlantUML.
- Chaque changement d’option relance la conversion en mémoire ; les onglets montrent le résultat, les avertissements et le résumé de la CLI. **Recharger** (F5) relit la source après une modification externe.
- **Mode strict** se comporte comme `--strict` : le résultat reste consultable, mais ne peut pas être enregistré tant que des avertissements existent.
- **Enregistrer sous…** (Ctrl+S) relit et reconvertit la source avant la publication atomique. Comme la CLI, l’interface refuse d’écrire sur la source ou sur un fichier existant : choisir un autre nom plutôt que remplacer un document.
- Raccourcis : Ctrl+1 à Ctrl+4 pour les onglets Résultat, Aperçu, Avertissements et Résumé ; Ctrl+Maj+C copie le résultat.

### Modifier le résultat

L’onglet **Résultat** est un éditeur : coloration syntaxique en direct (PlantUML, Atelier, XML SIMP), annulation illimitée (Ctrl+Z, Ctrl+Y), indentation automatique et numéros de ligne. Pour une sortie PlantUML, l’aperçu suit le texte modifié environ 0,3 s après la dernière frappe, et une erreur de syntaxe surligne la ligne fautive dans l’éditeur. **Enregistrer sous…**, **Copier** et **Exporter l’image…** utilisent la version modifiée ; elle est enregistrée telle quelle, sans reconversion ni mode strict, et sans jamais remplacer un fichier existant. « Rétablir la conversion » revient au texte généré. Changer une option, recharger ou ouvrir un autre fichier demande confirmation avant de remplacer des modifications.

Le menu **Mise en page** modifie le texte PlantUML (chaque action s’annule en une étape) :
- **Suivre la disposition SIMP** (coché par défaut) : décoché, les indications de position (directions des liens, liens invisibles) sont retirées et PlantUML place tout librement. Pour les retrouver : Ctrl+Z ou « Rétablir la conversion ».
- **Préréglage : page portrait** : disposition libre, hiérarchie de gauche à droite et notes coupées. Monopoly passe de ≈ 4 100 × 2 400 px (disposition SIMP) à ≈ 1 500 × 4 200 px, pour une page de document en portrait.
- Directives à cocher : hiérarchie de gauche à droite (`left to right direction`), notes coupées à 200 px (`skinparam wrapWidth 200`), compartiments vides masqués, liens à angle droit (`skinparam linetype ortho`, sans effet avec la mise en page intégrée `smetana` : il faut Graphviz).

Dans l’aperçu, « Ajuster » descend jusqu’à 10 % pour montrer un grand diagramme en entier.

### Aperçu et export d’images

L’onglet **Aperçu** dessine le diagramme sélectionné et se met à jour à chaque changement d’option, quel que soit le format cible : une source `.puml` est rendue telle quelle, une source SIMP est convertie en PlantUML en mémoire (le mode strict ne bloque pas l’aperçu). Zoom net (−, +, Ajuster, Taille réelle, Ctrl+molette), déplacement à la souris et navigation entre les pages quand « Tous les diagrammes » produit plusieurs vues. **Exporter l’image…** enregistre en PNG, PNG ×2, SVG, EPS ou texte ASCII ; plusieurs vues donnent `nom-1.png`, `nom-2.png`… Comme pour les conversions, aucun fichier existant n’est remplacé.

Le rendu est local : [PlantUML](https://plantuml.com) s’exécute sur l’ordinateur, rien n’est envoyé à un serveur. Le profil de sécurité `SANDBOX` interdit toute lecture de fichier local ou d’URL (le profil `SECURE` autorise encore `!include` de fichiers `.puml` locaux) ; la bibliothèque standard de PlantUML (`!include <C4/…>`) reste disponible. Sans Graphviz, la mise en page intégrée `smetana` est utilisée.

**Vitesse.** L’aperçu utilise l’exécutable **natif** de PlantUML (compilé, sans Java), publié pour Windows x64, Linux x64/arm64 et macOS Apple Silicon ; ailleurs (macOS Intel…), Java + plantuml.jar prend le relais. Deux processus PlantUML restent ouverts pendant la session (mode `-pipe`) : l’un calcule la mise en page en SVG pour connaître la taille, l’autre dessine le PNG directement à la résolution affichée. Ils démarrent avec la fenêtre, et l’aperçu est préparé dès chaque conversion, avant même l’ouverture de l’onglet. Un grand diagramme montre d’abord une ébauche à demi-résolution, puis la version nette ; un rendu devenu inutile (option changée entre-temps) est interrompu. Mesures sur Windows 11 : petit diagramme ≈ 10 ms au lieu de 1,3 s ; Monopoly (≈ 10 800 px de large) ébauche à 0,6 s et image nette à 1,6 s au lieu d’environ 4 s. Une erreur de syntaxe pendant l’édition laisse la dernière image valide affichée, avec le message et la ligne fautive. Les exports SVG passent par le processus ouvert ; les autres formats par un processus isolé.

Si le moteur manque, l’onglet propose de l’installer en un clic, dans le même dossier de données que les lanceurs : PlantUML 1.2026.9 natif (≈ 50 Mo) ou, à défaut, plantuml.jar (≈ 29 Mo) et si nécessaire Java 21 Eclipse Temurin (≈ 48 Mo, somme fournie par l’API Adoptium). Les sommes SHA-256 de PlantUML sont fixées dans `render.py`. `SIMP2PUML_PLANTUML` (exécutable natif), `PLANTUML_JAR`, `SIMP2PUML_JAVA` et `JAVA_HOME` permettent d’utiliser des installations existantes. PlantUML est téléchargé depuis ses publications officielles (licence GPL) et n’est pas redistribué avec ce projet.

Les proportions suivent le nombre d’or φ là où il sert la mise en page (constantes en tête de `gui.py`) : fenêtre et portrait en rectangle d’or, échelle typographique de raison √φ (8 à 26 pt), espacements de Fibonacci (3, 5, 8, 13, 21, 34 px), espace libre vertical réparti 1 : φ (écran d’accueil, carte d’installation, aperçu) et tuiles du résumé au rapport φ². L’icône reste carrée, comme l’exigent les systèmes.

L’icône et le portrait reproduisent *La Joconde* de Léonard de Vinci (domaine public), d’après la reproduction « Mona Lisa, by Leonardo da Vinci, from C2RMF retouched » de Wikimedia Commons. Les images de `src/simp2puml/assets/` se régénèrent avec `tools/build-icons.py` (Pillow requis pour cet outil uniquement).

## Export Atelier Diagrammes (Typst)

`--to atelier` produit une source DSL 2.0 (`dsl 2` en tête) pour le package Typst `atelier-diagrammes` 2.x :

```typst
#import "@local/atelier-diagrammes:2.1.0" as ad
#ad.render(read("diagramme.atelier"))
```

Le contenu peut aussi être collé dans un bloc ```` ```diagramme-ad ```` avec `#show raw.where(lang: "diagramme-ad"): ad.show-diagramme`. Correspondances :

- Les identifiants DSL sont dérivés des noms affichés (ASCII, mots-clés évités, suffixes `_2`… pour les homonymes) ; le nom d’origine reste le titre cité. Relations et notes utilisent des références absolues `::Nom`.
- Les membres sont écrits avec `field`/`method`, visibilité et `static`/`abstract` explicites ; un texte ambigu (accolades, guillemets, délimiteurs déséquilibrés, espaces de bord) devient une chaîne citée.
- Une énumération sans membres devient `enum` ; avec des attributs ou opérations, une `class … <<enumeration>>` dont les valeurs sont dans un compartiment `[kind=literal]`.
- Les symboles d’interface deviennent des `circle` reliés par association à leur fournisseur. L’héritage reste `--|>` : le modèle ne distingue pas la réalisation.
- Avertissements : visibilités d’extrémité (sans équivalent DSL), multiplicités hors `* | N | N..M | N..*` ou portées par un héritage, et notes sans cible, empilées à une position arbitraire car le DSL exige `at=(x,y)` pour une note libre.

Les sorties ont été vérifiées avec `atelier-diagrammes check` (2.1.0) sur les fixtures de test, pas sur tous les diagrammes possibles. Si `ATELIER_DIAGRAMMES_BIN` désigne ce binaire (ou s’il est dans le `PATH`), les tests `tests/test_atelier.py` valident aussi les sorties avec lui.

## Construire et paqueter

Construire une wheel localement dans un environnement isolé :

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements-dev.txt
.venv/bin/python -m build
```

Pour préparer un paquet natif local, le script prend en charge RPM et Debian :

```sh
.venv/bin/python tools/build-native.py rpm
# Sans les dépendances natives, produire seulement le paquet source RPM :
.venv/bin/python tools/build-native.py rpm --source-only
# Debian : remplacer le texte entre guillemets par une adresse publique réelle.
.venv/bin/python tools/build-native.py deb --maintainer-email "ADRESSE_PUBLIQUE_A_RENSEIGNER"
```

`ADRESSE_PUBLIQUE_A_RENSEIGNER` est un espace réservé, pas une adresse à publier. Sans adresse réelle, la préparation et la construction Debian sont volontairement refusées. Le nom du mainteneur Debian par défaut est Nela. Les recettes sont dans `packaging/rpm/simp2puml.spec.in` et `packaging/debian/`. `--source-only` construit un `.src.rpm`, pas un paquet binaire installable. `--prepare-only` prépare les sources sans lancer la construction ; `--output` permet de choisir le dossier de sortie (par défaut `dist`). Les outils de construction requis dépendent de la distribution. Pour RPM, prévoir notamment `rpmdevtools`, `rpmbuild`, `python3-devel`, `pyproject-rpm-macros` et setuptools ≥ 77. Pour Debian, prévoir `debhelper`, `dh-python`, `pybuild-plugin-pyproject`, `python3-all`, `python3-build`, `python3-wheel` et `python3-setuptools` ≥ 77. Les noms et disponibilités de paquets peuvent varier selon la version de la distribution ; aucune compatibilité de version non vérifiée n’est garantie.

Installer un paquet construit localement : `sudo dnf install ./dist/<paquet>.rpm` ou `sudo apt install ./dist/<paquet>.deb`. Cela n’installe pas le logiciel depuis un dépôt officiel et ne signifie ni publication PyPI, ni présence dans un dépôt APT/DNF, ni signature de dépôt. Sur les autres systèmes, utiliser pipx et la wheel ; aucune recette native supplémentaire n’est fournie.

## Couverture et limites

La priorité est SIMP vers PlantUML. Tous les types de vues XML d’un projet monofichier sont sélectionnables. **Accepter un type de diagramme ne signifie pas traduire intégralement toutes ses fonctionnalités dans sa notation native.** Les structures sans adaptateur dédié utilisent un graphe annoté et produisent des avertissements. Le format SIMP ne dispose pas ici d’un schéma exhaustif vérifié ; aucune couverture sémantique à 100 % n’est affirmée.

| Famille SIMP | Représentation produite |
| --- | --- |
| Classes | Classes, interfaces, énumérations, types de données, signaux, symboles d’interface, associations, agrégations/compositions, généralisations, dépendances et réalisations. Les vues enrichies ajoutent stéréotypes, valeurs initiales, paramètres directionnels et annotations de modificateurs. |
| Packages, composants, déploiement, structure composite | Packages, composants, nœuds, artefacts et conteneurs imbriqués ; liens orientés et stéréotypes. Un conteneur de forme non compatible devient un rectangle avec avertissement. |
| Cas d’utilisation | Acteurs, cas d’utilisation, généralisations, `include` et `extend`. Les scénarios textuels supplémentaires deviennent des annotations. |
| Objets | Instances avec nom de classifieur, valeurs des slots, liens et notes. |
| Entités-relations | Entités, attributs, clés, multiplicités textuelles. Les tailles de types et autres propriétés supplémentaires sont annotées ; aucune équivalence complète des contraintes SQL n’est annoncée. |
| États | États, transitions, gardes, effets, actions, états composites et principaux pseudo-états. |
| Séquence | Participants distincts, messages ordonnés par coordonnées verticales, appels synchrones/asynchrones, retours, création/destruction, activations et fragments `alt`, `opt`, `loop`, `par`, `break`, `critical`. |
| Activités | Graphe de flux orienté en notation d’états, avec décisions/fourches/jonctions ; symboles d’action et partitions approximés, avec avertissement. |
| Autres familles (BPMN, SysML, ArchiMate, interface utilisateur, organigrammes, etc.) | Graphe générique annoté : éléments nommés, conteneurs et liens lorsque leurs extrémités sont définies. La notation et les règles propres à ces familles ne sont pas implémentées. |

Les identifiants internes sont remplacés par des alias sûrs, sans fusionner les homonymes. Les rôles et multiplicités restent attachés à leur extrémité. Dans les vues enrichies, la navigabilité positive devient une flèche ; la non-navigabilité explicite, les références `attribute-ref` et les valeurs de navigabilité inconnues sont signalées et archivées. Les classes d’association sont reliées à leur association binaire lorsque ces extrémités sont présentes.

Les définitions partagées au niveau du projet ou d’un modèle sont incluses lorsqu’elles sont présentées dans le diagramme sélectionné. Les définitions locales d’un autre diagramme ne sont pas importées par cette sélection.

Les profils XML `10` (exemple livré avec Software Ideas Modeler) et `12.75` sont observés. Les autres numéros numériques passent par la lecture structurelle avec avertissement de profil non validé. Le numéro XML n’indique pas la version de l’application. Les projets multifichiers, chiffrés ou non XML restent refusés ; aucun fichier externe n’est chargé. Les références inexistantes, identités dupliquées et cycles de conteneurs restent des erreurs.

PlantUML recalcule les styles et les positions ; les positions SIMP orientent toutefois la mise en page des vues structurelles. Chaque lien reçoit une direction (`-up-`, `-left-`…) qui reprend la position relative de ses extrémités dans SIMP. Deux éléments empilés dans SIMP sans lien entre eux reçoivent un lien invisible (`-[hidden]down-`) qui garde la colonne ; les éléments sans aucun lien (interfaces, énumérations) forment leurs propres colonnes, sans être rattachés au graphe principal. Monopoly mesure ainsi ≈ 4 100 × 2 400 px (proportion 1,72 ; 1,63 dans SIMP), au lieu de ≈ 10 800 × 1 700 px sans ces indications. Graphviz a été comparé sur ce diagramme : plus large (≈ 5 900 à 6 800 px), il n’apporte rien ici et n’est pas nécessaire. Une note reliée à un seul élément est accrochée de son côté dans SIMP (`note left of …`) et coupée à la largeur de sa boîte. Un symbole d’interface relié une seule fois porte son nom sur son lien et reste au rang voisin de sa classe. La visibilité d’une extrémité d’association n’est affichée qu’avec un nom de rôle. Un classifieur générique (`IComparable<T>`) utilise la notation UML de PlantUML, si ses paramètres ne contiennent que des caractères sans effet ; sinon le nom reste échappé. Dans les vues enrichies, le XML des éléments sélectionnés, des occurrences et du contexte de vue est conservé dans des commentaires JSON `simp2puml-source`/`simp2puml-context`. C’est une archive consultable, **pas une conversion sémantique ni un mécanisme d’import inverse**. Les propriétés supplémentaires détectées sont affichées en annotation avec avertissement. L’archive peut contenir les mêmes informations sensibles que le fichier SIMP source ; traiter la sortie comme ce fichier. Les notes et noms sont échappés pour rester du texte, sans directives, liens ou images chargées.

En séquence, les occurrences multiples d’une même identité sont réunies avec avertissement. Sans coordonnées de message, l’ordre XML sert de repli signalé. Les fragments croisés deviennent des séparateurs, et ceux sans intervalle graphique des notes. Les activations sont ancrées aux messages ; celles sans message intermédiaire sont omises du dessin, celles imbriquées sur une même ligne de vie sont réunies, avec avertissement et archive XML. Les durées exactes ne sont pas restituées.

Le lecteur PlantUML accepte un sous-ensemble strict, avec une instruction par ligne et des membres de la forme `nom: Type` ou `operation(parametre: Type): Retour`. Les packages, réalisations, valeurs par défaut, stéréotypes arbitraires et préprocesseur ne sont pas pris en charge. Les directives graphiques reconnues sont ignorées avec avertissement ; les autres constructions non prises en charge sont refusées. Aucun fichier externe n’est chargé. Voir [l’exemple minimal](examples/minimal.puml).

L’export SIMP reste expérimental et limité au modèle historique de classes ; il génère une grille. L’export Atelier et l’import PlantUML ne sont pas élargis à toutes les familles : une vue SIMP enrichie est explicitement refusée par `write_atelier`/`write_simp`, plutôt que de produire une sortie vide. Les flèches d’association lues depuis PlantUML perdent encore leur direction avec avertissement. Le lecteur PlantUML interne ne peut donc pas relire toutes les sorties enrichies ; utiliser PlantUML lui-même pour les vérifier.

`--strict` refuse les approximations et pertes détectées, y compris le recalcul de mise en page signalé par les vues enrichies. Un avertissement peut concerner plusieurs éléments ; leur nombre n’est pas un décompte des éléments perdus. Vérifier la sortie dans l’outil cible avant de remplacer un document de travail.

## Éditeur PlantUML (GPUI) et rendu natif (Nim)

Le dépôt contient aussi un éditeur PlantUML de bureau, indépendant du paquet Python :

- `editor/` : application Rust construite avec [GPUI](https://gpui.rs) (le cadriciel d'interface de Zed). Onglets, coloration syntaxique, autocomplétion (mots-clés selon le contexte, noms déclarés dans le document, flèches, extraits, avec correspondance approximative), numéros de ligne, annulation/rétablissement, recherche et remplacement, commentaire de lignes, duplication et déplacement de lignes, indentation automatique, modèles de diagrammes, diagnostics par ligne, et aperçu **vectoriel** en direct : le SVG produit par le moteur est dessiné directement avec les primitives GPUI (formes, chemins, texte), sans image bitmap, et reste net à tout zoom. Un clic sur un élément du diagramme place le curseur sur la ligne correspondante ; Ctrl+molette zoome autour du pointeur, la molette ou un glisser fait défiler.
- `renderer/` : moteur `pumlrender` écrit en Nim, qui produit le SVG de l'aperçu sans Java ni PlantUML. Il couvre les diagrammes de classes, d'objets, de séquence, d'activité (nouvelle syntaxe, et ancienne syntaxe `(*)` sous forme simplifiée), d'états, de cas d'utilisation, de composants/déploiement, les cartes mentales, WBS, Gantt, JSON et YAML. Les autres types (`salt`, `timing`, `ditaa`…) restent éditables mais affichent un aperçu de remplacement avec un avertissement.

Organisation de l'éditeur (MVVM) :

| Couche | Dossier | Rôle |
| --- | --- | --- |
| Modèle | `editor/src/model/` | tampon de texte, coloration, autocomplétion, résultat de rendu, scène vectorielle lue depuis le SVG, modèles de diagrammes ; aucune dépendance à l'interface |
| Services | `editor/src/services/` | processus `pumlrender`, fichiers et convertisseur `simp2puml` |
| ViewModel | `editor/src/viewmodel/` | état présenté et commandes (édition, document et rendu, espace de travail) ; émet des évènements, ne dessine rien |
| Vue | `editor/src/view/` | dessin GPUI et traduction clavier/souris en commandes ; dialogues et focus |

Optimisations inspirées de Zed : processus de rendu persistant (`pumlrender --serve`, comme un serveur de langage) au lieu d'un processus par frappe ; rendu différé de 150 ms où seul le dernier résultat compte ; lecture du SVG et construction de la scène dans l'exécuteur d'arrière-plan ; index des lignes mis à jour de façon incrémentale ; coloration incrémentale et paresseuse ; seules les lignes visibles de l'éditeur et les éléments visibles de l'aperçu sont dessinés ; liste des diagnostics virtualisée. Un rendu qui dépasse 5 s est abandonné et le moteur relancé (un document qui le bloquerait ne fige pas les autres onglets) ; sous Linux, le moteur s'arrête avec l'éditeur.

Le rendu natif n'est pas identique à celui de PlantUML : mise en page, styles et `skinparam` diffèrent, et une syntaxe non reconnue est signalée puis ignorée. Au-delà de 200 niveaux d'imbrication, un diagramme est refusé avec un diagnostic. Le préprocesseur PlantUML (`!include`, `!define`, `!theme`…) n'est jamais exécuté : ces lignes sont signalées et ignorées, et aucun fichier externe n'est chargé.

Construire (Nim ≥ 1.6, Rust stable ; sous Linux, GPUI requiert notamment les en-têtes `libxkbcommon`, `wayland`, `vulkan`, `fontconfig` et `xcb`) :

```sh
cd renderer && nimble build        # ou : nim c -o:bin/pumlrender src/pumlrender.nim
cd ../editor && cargo build --release
./target/release/puml-editor diagramme.puml
```

L'éditeur cherche `pumlrender` dans `$PUMLRENDER`, à côté de l'exécutable ou dans `renderer/bin/` d'un dossier parent (dépôt), puis dans les entrées absolues du `PATH`. Le moteur s'utilise aussi seul : `pumlrender diagramme.puml -o diagramme.svg` (une sortie existante, même un lien symbolique, est refusée), `pumlrender --check diagramme.puml` pour les seuls diagnostics. `--serve` est le mode persistant utilisé par l'éditeur.

Fichiers : « Enregistrer » remplace atomiquement le fichier ouvert (un lien symbolique reste un lien, les droits sont conservés, un fichier en lecture seule est refusé) et demande confirmation si un autre programme l'a modifié depuis l'ouverture ; un nom de sortie saisi sans extension reçoit `.puml`, `.svg` ou `.simp` ; « Enregistrer sous », « Exporter SVG » et « Exporter SIMP » visent l'onglet actif au moment de la demande et refusent d'écraser un fichier existant ; un même fichier n'est ouvert que dans un onglet. « Exporter SVG » n'exporte qu'un aperçu à jour. « Importer SIMP » (ou l'ouverture d'un `.simp`) passe par le convertisseur `simp2puml` (`$SIMP2PUML`, à côté de l'exécutable, `PATH`, sinon `python3 -P -m simp2puml`) et ouvre le résultat dans un nouvel onglet non enregistré (sa fermeture demande confirmation), sans rien écrire à côté de la source ; chaque diagramme du projet y devient un bloc `@start…@end` (`--all`). L'export SIMP reste expérimental et limité au sous-ensemble strict de diagrammes de classes décrit plus haut. Sans portail XDG pour les sélecteurs de fichiers, l'éditeur propose une saisie de chemin.

Raccourcis principaux (`Ctrl` sous Linux/Windows, `Cmd` sous macOS) : `Ctrl+N/O/S`, `Ctrl+Maj+S`, `Ctrl+W`, `Ctrl+Tab`/`Ctrl+PgUp/PgDn` (onglets), `Ctrl+F` ou `Ctrl+H` (Entrée/F3/`Ctrl+G` suivant ; Entrée dans le champ de remplacement remplace l'occurrence sélectionnée puis passe à la suivante), `Ctrl+Z`/`Ctrl+Y` ou `Ctrl+Maj+Z`, `Ctrl+I` (importer SIMP), `Ctrl+Q` (quitter), `Ctrl+,` (préférences), `Ctrl+/` (commenter), `Ctrl+D` (dupliquer), `Ctrl+Maj+K` (supprimer la ligne), `Alt+↑/↓` (déplacer), `Ctrl+E` (export SVG), `Ctrl+=`/`Ctrl+-`/`Ctrl+0`/`Ctrl+9` (zoom, ajuster), `Ctrl+J` (diagnostics), `Ctrl+T` (modèles), `Alt+PgUp/PgDn` (bloc @start précédent/suivant). Autocomplétion : le menu s'ouvre en tapant (ou `Ctrl+Espace`), `↑/↓` pour choisir, `Entrée`/`Tab` pour insérer, `Échap` pour fermer.

Préférences (engrenage ⚙ de la barre d'outils ou `Ctrl+,`) : thème Système, Clair ou Sombre ; en thème sombre, diagramme sur feuille claire ou couleurs adaptées (luminosité inversée, teintes conservées ; l'export SVG reste inchangé) ; autocomplétion pendant la frappe ou seulement avec `Ctrl+Espace` ; taille du texte de l'éditeur (10 à 28). Chaque choix s'applique et s'enregistre aussitôt dans `$XDG_CONFIG_HOME/puml-editor/preferences.conf` (par défaut `~/.config/puml-editor/`), un fichier `clé = valeur` commenté ; une ligne invalide est signalée puis ignorée.

Tests : `cd renderer && nimble test` (ou `nim c -r --path:src tests/test_render.nim`) et `cd editor && cargo test`. Ces derniers comprennent des parcours d'interface (`editor/src/view/interface_tests.rs`) : clics, raccourcis et dialogues réels sur la plateforme de test de GPUI, sans fenêtre ni session graphique.

## Développement

Installer avec `python3 -m venv .venv` puis `.venv/bin/python -m pip install -r requirements-dev.txt`. Tests ciblés : `.venv/bin/python -m pytest -q tests/test_simp.py`, `test_puml.py` ou `test_cli.py`. Exécution : `.venv/bin/python -m simp2puml fichier.simp`. L’ancien `main.py` est conservé comme wrapper de compatibilité.

La suite complète se lance avec `.venv/bin/python -m pytest -q`. La fixture réelle `Monopoly.simp` et les exports personnels ne sont pas redistribués : seuls les tests qui en dépendent sont ignorés lorsqu’elle manque. Les tests synthétiques ne garantissent pas la compatibilité avec l’application SIMP.

`tests/test_simp_views.py` couvre les adaptateurs, la sélection, l’export de projet, les références, les conteneurs et l’injection de directives. Avec `PLANTUML_JAR` pointant vers un JAR local, ces sorties passent aussi la vérification de syntaxe du moteur officiel (profil de sécurité `SECURE`). `SIMP_REFERENCE_FILE` permet de fournir un projet réel local : chaque vue est vérifiée, puis `--all` est rendu en SVG et le nombre de dessins est contrôlé. La validation locale utilise PlantUML 1.2026.2 et les neuf vues d’`ApplicationStore.simp` livré avec l’application ; aucun de ces fichiers tiers n’est redistribué.

API : `list_diagrams(text)` renvoie les tuples `(id, name, type)`. `parse_simp` conserve les dataclasses du sous-ensemble de classes historique ; pour une vue enrichie, `UMLDiagram.source_view` contient les éléments/occurrences XML et les collections historiques ne constituent pas une projection de cette vue. `write_puml` choisit l’adaptateur adapté. Les diagnostics d’export sont ajoutés à `diagram.warnings` sans doublon lors d’écritures répétées.

Les correctifs du moteur restent en Python pour préserver l’API importable, les dataclasses publiques et l’installation en wheel sans dépendance native. Un moteur Nim distinct nécessiterait une migration de cette API ou le maintien de deux implémentations ; cette évolution ne fait pas partie des corrections actuelles.
