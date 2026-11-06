# Moteur de rendu natif

Le moteur `pumlrender` (`renderer/`), écrit en Nim, produit le SVG de l'aperçu sans Java ni PlantUML. Il couvre les diagrammes de classes, d'objets, de séquence, d'activité (nouvelle syntaxe, et ancienne syntaxe `(*)` sous forme simplifiée), d'états, de cas d'utilisation, de composants/déploiement, les cartes mentales, WBS, Gantt, JSON et YAML. Un type non reconnu reste éditable mais affiche un aperçu de remplacement avec un avertissement.

Le rendu natif n'est pas identique à celui de PlantUML : la mise en page diffère et une syntaxe non reconnue est signalée puis ignorée. Il comprend aussi `salt`, `timing`, `ditaa`, `archimate`, les sprites hexadécimaux, le `&` des séquences, le créole courant (`**gras**`, `//italique//`, `<color:…>`), `skinparam`, `<style>` et `!theme` (palettes relevées sur PlantUML, sans leurs effets de dégradé).

Le préprocesseur est un sous-ensemble sûr et borné : `!define`, `!definelong`, variables, `!if`, `!while`, `!procedure`, `!function` et quelques fonctions pures ; le moteur ne lit jamais de fichier ni l'environnement. Au-delà de 200 niveaux d'imbrication, un diagramme est refusé avec un diagnostic.

## Utiliser le moteur seul

L'éditeur cherche `pumlrender` dans `$PUMLRENDER`, à côté de l'exécutable ou dans `renderer/bin/` d'un dossier parent (dépôt), puis dans les entrées absolues du `PATH`. Le moteur s'utilise aussi seul : `pumlrender diagramme.puml -o diagramme.svg` (une sortie existante, même un lien symbolique, est refusée), `pumlrender --check diagramme.puml` pour les seuls diagnostics. `--serve` est le mode persistant utilisé par l'éditeur.

## Performances

Optimisations inspirées de Zed : processus de rendu persistant (`pumlrender --serve`, comme un serveur de langage) au lieu d'un processus par frappe ; rendu immédiat d'une frappe isolée, différé dans une rafale de deux fois la durée du rendu précédent (16 à 150 ms), où seul le dernier résultat compte et sans file de rendus périmés ; lecture du SVG et construction de la scène dans l'exécuteur d'arrière-plan ; index des lignes mis à jour de façon incrémentale ; coloration incrémentale et paresseuse ; seules les lignes visibles de l'éditeur et les éléments visibles de l'aperçu sont dessinés ; liste des diagnostics virtualisée.

Un rendu qui dépasse 5 s est abandonné et le moteur relancé (un document qui le bloquerait ne fige pas les autres onglets) ; sous Linux, le moteur s'arrête avec l'éditeur.

Mesures sur un Ryzen 5 7520U, sur batterie et à 60 Hz, entre la version d'avant ces optimisations et la version actuelle, en passes alternées. Les diagrammes sont synthétiques et l'éditeur était piloté par ses ViewModels, sans vraie souris ni vrai clavier ; ces chiffres valent pour cette machine.

| Mesure | 12 classes | 150 classes | 500 classes |
| --- | --- | --- | --- |
| Survol, temps par image | 4,7 → **1,9 ms** | 49 → **1,9 ms** | 160 → **2,0 ms** |
| Frappe → aperçu à jour | 167 → **17 ms** | 215 → **50 ms** | 328 → **166 ms** |

Après une rafale de frappes sur 2 000 classes, l'aperçu est à jour en 0,6 à 0,8 s, contre 3,4 à 4,5 s. Le moteur seul rend 500 classes en 60 ms et 2 000 classes en 281 ms.

Une optimisation n'est adoptée que si des mesures répétées et alternées la montrent au-delà du bruit, sans dégrader le reste. Les vues `cached` de GPUI et mimalloc ont été écartés ainsi : les premières dégradent les pires images au survol, le second double la mémoire.
