# Export vers Atelier Diagrammes

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
