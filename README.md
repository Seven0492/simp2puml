# simp2puml

Outil en ligne de commande pour convertir des diagrammes SIMP vers PlantUML, et expérimentalement de PlantUML vers SIMP. Python ≥ 3.11 ; aucune dépendance tierce à l’exécution. Le modèle commun conserve les identités, classifieurs, membres, relations, notes et avertissements, sans reproduire fidèlement toutes les données du format source.

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
simp2puml diagramme.simp --check
simp2puml diagramme.simp --strict -o resultat.puml
```

L’extension source choisit le sens de conversion ; sans `-o`, le fichier de destination prend l’extension opposée dans le même dossier. `--diagram` sélectionne un diagramme SIMP par nom ou identifiant ; les diagrammes ne sont pas fusionnés. Une sortie existante ou identique à la source est refusée. La source est limitée à 20 MiB, lue en UTF-8 (BOM accepté) ; la sortie est publiée atomiquement.

`--check` effectue la lecture et la génération du format cible en mémoire, sans créer ni modifier de fichier. Il permet de vérifier la conversion même si la sortie par défaut existe déjà ; il ne s’utilise pas avec `-o`. `--strict` refuse toute conversion accompagnée d’avertissements, avant d’écrire la sortie. Les deux options se combinent pour une validation automatisée :

```sh
simp2puml projet.simp --diagram "Nom du diagramme" --check --strict
```

Le mode strict refuse aussi toute génération SIMP, car celle-ci émet toujours un avertissement expérimental. Il ne prouve pas l’absence de toute perte : seuls les avertissements détectés font échouer la commande. Codes de sortie : `0` pour une conversion ou vérification réussie, `1` pour une erreur de conversion, d’entrée/sortie ou un refus strict, `2` pour des arguments invalides.

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

La priorité de l’outil est SIMP vers PlantUML. Le lecteur SIMP vise le profil `sim-project` observé (XML `version="12.75"`, `multi-file="no"`) ; 12.75 est un numéro de format et ne permet pas de connaître la version de l’application. La compatibilité avec Software Ideas Modeler n’a pas été vérifiée. Une sélection explicite du diagramme évite de fusionner les diagrammes ; les objets globaux hors diagramme sélectionné sont exclus.

Le convertisseur conserve, dans le sous-ensemble pris en charge :

- Les classes, classes abstraites, interfaces, énumérations et leurs valeurs ordonnées, ainsi que les symboles d’interface distincts.
- Les attributs, opérations et paramètres : types, visibilités, modificateurs et multiplicités lorsqu’ils sont représentables.
- Les associations, agrégations, compositions, généralisations et connecteurs d’interface fournie : rôles et multiplicités d’extrémité compris.
- Les notes, leur texte et leurs liens aux classes. Les classes homonymes gardent des identités distinctes.

Les définitions partagées au niveau du projet ou d’un modèle sont incluses lorsqu’elles sont présentées dans le diagramme sélectionné. Les définitions locales d’un autre diagramme ne sont pas importées par cette sélection.

Les styles et coordonnées d’origine ne sont pas conservés. L’export SIMP génère une disposition en grille ; un aller-retour ne restaure donc pas la mise en page initiale. Les flèches d’association PlantUML perdent leur direction avec avertissement. Un avertissement peut concerner plusieurs éléments ; leur nombre n’est pas un décompte des éléments perdus.

Le lecteur PlantUML accepte un sous-ensemble strict, avec une instruction par ligne et des membres de la forme `nom: Type` ou `operation(parametre: Type): Retour`. Les packages, réalisations, valeurs par défaut, stéréotypes arbitraires et préprocesseur ne sont pas pris en charge. Les directives graphiques reconnues sont ignorées avec avertissement ; les autres constructions non prises en charge sont refusées. Aucun fichier externe n’est chargé. Voir [l’exemple minimal](examples/minimal.puml).

En cas d’échec, le diagnostic PlantUML indique la ligne lorsqu’elle est identifiable. Pour un projet SIMP contenant plusieurs diagrammes de classes, le message d’erreur énumère les noms et identifiants disponibles : relancer avec `--diagram`. Vérifier la sortie dans l’outil cible avant de remplacer un document de travail.

## Développement

Installer avec `python3 -m venv .venv` puis `.venv/bin/python -m pip install -r requirements-dev.txt`. Tests ciblés : `.venv/bin/python -m pytest -q tests/test_simp.py`, `test_puml.py` ou `test_cli.py`. Exécution : `.venv/bin/python -m simp2puml fichier.simp`. L’ancien `main.py` est conservé comme wrapper de compatibilité.

La suite complète se lance avec `.venv/bin/python -m pytest -q`. La fixture réelle `Monopoly.simp` et les exports personnels ne sont pas redistribués : seuls les tests qui en dépendent sont ignorés lorsqu’elle manque. Les tests synthétiques ne garantissent pas la compatibilité avec l’application SIMP.

Les correctifs du moteur restent en Python pour préserver l’API importable, les dataclasses publiques et l’installation en wheel sans dépendance native. Un moteur Nim distinct nécessiterait une migration de cette API ou le maintien de deux implémentations ; cette évolution ne fait pas partie des corrections actuelles.
