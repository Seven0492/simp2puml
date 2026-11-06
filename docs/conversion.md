# Conversion en ligne de commande

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

L’extension source choisit le sens de conversion ; sans `-o` ni `--to`, le fichier de destination prend l’extension opposée dans le même dossier. `--to puml|simp|atelier` choisit explicitement le format cible ; sans `--to`, une sortie `-o` en `.atelier` le sélectionne aussi. Le format cible doit différer du format source.

`--diagram` sélectionne un diagramme SIMP de tout type par nom ou identifiant. Sans sélection, un projet comportant plusieurs diagrammes est refusé. `--list-diagrams` affiche une ligne JSON par diagramme (identifiant, nom et type), sans conversion. `--all` produit un fichier `.puml` contenant un bloc `@startuml` distinct par vue, dans l’ordre du projet ; les diagrammes ne sont pas fusionnés.

Toutes les conversions doivent réussir avant publication du fichier. Une sortie existante ou identique à la source est refusée. La source est limitée à 20 MiB, lue en UTF-8 (BOM accepté) ; la sortie est publiée atomiquement.

`--check` effectue la lecture et la génération du format cible en mémoire, sans créer ni modifier de fichier. Il permet de vérifier la conversion même si la sortie par défaut existe déjà ; il ne s’utilise pas avec `-o`. `--strict` refuse toute conversion accompagnée d’avertissements, avant d’écrire la sortie. Les deux options se combinent pour une validation automatisée :

```sh
simp2puml projet.simp --diagram "Nom du diagramme" --check --strict
```

Le mode strict refuse aussi toute génération SIMP, car celle-ci émet toujours un avertissement expérimental. Il ne prouve pas l’absence de toute perte : seuls les avertissements détectés font échouer la commande. Codes de sortie : `0` pour une conversion ou vérification réussie, `1` pour une erreur de conversion, d’entrée/sortie ou un refus strict, `2` pour des arguments invalides.

Les conversions possibles et leurs limites sont détaillées dans [Couverture et limites](couverture.md).
