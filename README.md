# simp2puml

Outil en ligne de commande pour convertir des diagrammes SIMP vers PlantUML, et expérimentalement de PlantUML vers SIMP. Les deux formats s’exportent aussi vers le DSL 2.0 d’[Atelier Diagrammes](https://git.sethmartin.dev/Seth_Martin/atelier-diagrammes) (blocs Typst `diagramme-ad`). Python ≥ 3.11 ; aucune dépendance tierce à l’exécution. Le modèle commun conserve les identités, classifieurs, membres, relations, notes et avertissements, sans reproduire fidèlement toutes les données du format source.

## Origine et crédits

Ce projet reprend le convertisseur créé par **ASaki**, disponible dans le [dépôt GitHub d’origine de Nela](https://github.com/2534296-Mailloux-Nela/simp2puml). Merci à ASaki pour le travail initial sur la conversion SIMP et PlantUML, la CLI et le packaging.

Cette version ajoute des corrections de fidélité, des options de vérification et des diagnostics plus précis. La [licence MIT](LICENSE) et la mention de copyright d’origine de Nela sont conservées.

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

PlantUML recalcule les styles et les positions. Dans les vues enrichies, le XML des éléments sélectionnés, des occurrences et du contexte de vue est conservé dans des commentaires JSON `simp2puml-source`/`simp2puml-context`. C’est une archive consultable, **pas une conversion sémantique ni un mécanisme d’import inverse**. Les propriétés supplémentaires détectées sont affichées en annotation avec avertissement. L’archive peut contenir les mêmes informations sensibles que le fichier SIMP source ; traiter la sortie comme ce fichier. Les notes et noms sont échappés pour rester du texte, sans directives, liens ou images chargées.

En séquence, les occurrences multiples d’une même identité sont réunies avec avertissement. Sans coordonnées de message, l’ordre XML sert de repli signalé. Les fragments croisés deviennent des séparateurs, et ceux sans intervalle graphique des notes. Les activations sont ancrées aux messages ; celles sans message intermédiaire sont omises du dessin, celles imbriquées sur une même ligne de vie sont réunies, avec avertissement et archive XML. Les durées exactes ne sont pas restituées.

Le lecteur PlantUML accepte un sous-ensemble strict, avec une instruction par ligne et des membres de la forme `nom: Type` ou `operation(parametre: Type): Retour`. Les packages, réalisations, valeurs par défaut, stéréotypes arbitraires et préprocesseur ne sont pas pris en charge. Les directives graphiques reconnues sont ignorées avec avertissement ; les autres constructions non prises en charge sont refusées. Aucun fichier externe n’est chargé. Voir [l’exemple minimal](examples/minimal.puml).

L’export SIMP reste expérimental et limité au modèle historique de classes ; il génère une grille. L’export Atelier et l’import PlantUML ne sont pas élargis à toutes les familles : une vue SIMP enrichie est explicitement refusée par `write_atelier`/`write_simp`, plutôt que de produire une sortie vide. Les flèches d’association lues depuis PlantUML perdent encore leur direction avec avertissement. Le lecteur PlantUML interne ne peut donc pas relire toutes les sorties enrichies ; utiliser PlantUML lui-même pour les vérifier.

`--strict` refuse les approximations et pertes détectées, y compris le recalcul de mise en page signalé par les vues enrichies. Un avertissement peut concerner plusieurs éléments ; leur nombre n’est pas un décompte des éléments perdus. Vérifier la sortie dans l’outil cible avant de remplacer un document de travail.

## Développement

Installer avec `python3 -m venv .venv` puis `.venv/bin/python -m pip install -r requirements-dev.txt`. Tests ciblés : `.venv/bin/python -m pytest -q tests/test_simp.py`, `test_puml.py` ou `test_cli.py`. Exécution : `.venv/bin/python -m simp2puml fichier.simp`. L’ancien `main.py` est conservé comme wrapper de compatibilité.

La suite complète se lance avec `.venv/bin/python -m pytest -q`. La fixture réelle `Monopoly.simp` et les exports personnels ne sont pas redistribués : seuls les tests qui en dépendent sont ignorés lorsqu’elle manque. Les tests synthétiques ne garantissent pas la compatibilité avec l’application SIMP.

`tests/test_simp_views.py` couvre les adaptateurs, la sélection, l’export de projet, les références, les conteneurs et l’injection de directives. Avec `PLANTUML_JAR` pointant vers un JAR local, ces sorties passent aussi la vérification de syntaxe du moteur officiel (profil de sécurité `SECURE`). `SIMP_REFERENCE_FILE` permet de fournir un projet réel local : chaque vue est vérifiée, puis `--all` est rendu en SVG et le nombre de dessins est contrôlé. La validation locale utilise PlantUML 1.2026.2 et les neuf vues d’`ApplicationStore.simp` livré avec l’application ; aucun de ces fichiers tiers n’est redistribué.

API : `list_diagrams(text)` renvoie les tuples `(id, name, type)`. `parse_simp` conserve les dataclasses du sous-ensemble de classes historique ; pour une vue enrichie, `UMLDiagram.source_view` contient les éléments/occurrences XML et les collections historiques ne constituent pas une projection de cette vue. `write_puml` choisit l’adaptateur adapté. Les diagnostics d’export sont ajoutés à `diagram.warnings` sans doublon lors d’écritures répétées.

Les correctifs du moteur restent en Python pour préserver l’API importable, les dataclasses publiques et l’installation en wheel sans dépendance native. Un moteur Nim distinct nécessiterait une migration de cette API ou le maintien de deux implémentations ; cette évolution ne fait pas partie des corrections actuelles.
