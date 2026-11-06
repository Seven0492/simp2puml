# Développement

## Organisation de l'éditeur

Organisation de l'éditeur (MVVM) :

| Couche | Dossier | Rôle |
| --- | --- | --- |
| Modèle | `editor/src/model/` | tampon de texte, coloration, autocomplétion, résultat de rendu, scène vectorielle lue depuis le SVG, modèles de diagrammes ; aucune dépendance à l'interface |
| Services | `editor/src/services/` | processus `pumlrender`, fichiers et convertisseur `simp2puml` |
| ViewModel | `editor/src/viewmodel/` | état présenté et commandes (édition, document et rendu, espace de travail) ; émet des évènements, ne dessine rien |
| Vue | `editor/src/view/` | dessin GPUI et traduction clavier/souris en commandes ; dialogues et focus |

## Tests

Tests : `cd renderer && nimble test` (ou `nim c -r --path:src tests/test_render.nim`) et `cd editor && cargo test`. Ces derniers comprennent des parcours d'interface (`editor/src/view/interface_tests.rs`) : clics, raccourcis et dialogues réels sur la plateforme de test de GPUI, sans fenêtre ni session graphique.

## Paquet Python

Installer avec `python3 -m venv .venv` puis `.venv/bin/python -m pip install -r requirements-dev.txt`. Tests ciblés : `.venv/bin/python -m pytest -q tests/test_simp.py`, `test_puml.py` ou `test_cli.py`. Exécution : `.venv/bin/python -m simp2puml fichier.simp`. L’ancien `main.py` est conservé comme wrapper de compatibilité.

La suite complète se lance avec `.venv/bin/python -m pytest -q`. La fixture réelle `Monopoly.simp` et les exports personnels ne sont pas redistribués : seuls les tests qui en dépendent sont ignorés lorsqu’elle manque. Les tests synthétiques ne garantissent pas la compatibilité avec l’application SIMP.

`tests/test_simp_views.py` couvre les adaptateurs, la sélection, l’export de projet, les références, les conteneurs et l’injection de directives. Avec `PLANTUML_JAR` pointant vers un JAR local, ces sorties passent aussi la vérification de syntaxe du moteur officiel (profil de sécurité `SECURE`). `SIMP_REFERENCE_FILE` permet de fournir un projet réel local : chaque vue est vérifiée, puis `--all` est rendu en SVG et le nombre de dessins est contrôlé. La validation locale utilise PlantUML 1.2026.2 et les neuf vues d’`ApplicationStore.simp` livré avec l’application ; aucun de ces fichiers tiers n’est redistribué.

API : `list_diagrams(text)` renvoie les tuples `(id, name, type)`. `parse_simp` conserve les dataclasses du sous-ensemble de classes historique ; pour une vue enrichie, `UMLDiagram.source_view` contient les éléments/occurrences XML et les collections historiques ne constituent pas une projection de cette vue. `write_puml` choisit l’adaptateur adapté. Les diagnostics d’export sont ajoutés à `diagram.warnings` sans doublon lors d’écritures répétées.

Les correctifs du moteur restent en Python pour préserver l’API importable, les dataclasses publiques et l’installation en wheel sans dépendance native. Un moteur Nim distinct nécessiterait une migration de cette API ou le maintien de deux implémentations ; cette évolution ne fait pas partie des corrections actuelles.

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

`ADRESSE_PUBLIQUE_A_RENSEIGNER` est un espace réservé, pas une adresse à publier. Sans adresse réelle, la préparation et la construction Debian sont volontairement refusées. Le nom du mainteneur Debian par défaut est Nela. Les recettes sont dans `packaging/rpm/simp2puml.spec.in` et `packaging/debian/`. `--source-only` construit un `.src.rpm`, pas un paquet binaire installable.

`--prepare-only` prépare les sources sans lancer la construction ; `--output` permet de choisir le dossier de sortie (par défaut `dist`). Les outils de construction requis dépendent de la distribution. Pour RPM, prévoir notamment `rpmdevtools`, `rpmbuild`, `python3-devel`, `pyproject-rpm-macros` et setuptools ≥ 77.

Pour Debian, prévoir `debhelper`, `dh-python`, `pybuild-plugin-pyproject`, `python3-all`, `python3-build`, `python3-wheel` et `python3-setuptools` ≥ 77. Les noms et disponibilités de paquets peuvent varier selon la version de la distribution ; aucune compatibilité de version non vérifiée n’est garantie.

Installer un paquet construit localement : `sudo dnf install ./dist/<paquet>.rpm` ou `sudo apt install ./dist/<paquet>.deb`. Cela n’installe pas le logiciel depuis un dépôt officiel et ne signifie ni publication PyPI, ni présence dans un dépôt APT/DNF, ni signature de dépôt. Sur les autres systèmes, utiliser pipx et la wheel ; aucune recette native supplémentaire n’est fournie.
