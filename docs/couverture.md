# Couverture et limites

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

PlantUML recalcule les styles et les positions ; les positions SIMP orientent toutefois la mise en page des vues structurelles. Chaque lien reçoit une direction (`-up-`, `-left-`…) qui reprend la position relative de ses extrémités dans SIMP. Deux éléments empilés dans SIMP sans lien entre eux reçoivent un lien invisible (`-[hidden]down-`) qui garde la colonne ; les éléments sans aucun lien (interfaces, énumérations) forment leurs propres colonnes, sans être rattachés au graphe principal.

Monopoly mesure ainsi ≈ 4 100 × 2 400 px (proportion 1,72 ; 1,63 dans SIMP), au lieu de ≈ 10 800 × 1 700 px sans ces indications. Graphviz a été comparé sur ce diagramme : plus large (≈ 5 900 à 6 800 px), il n’apporte rien ici et n’est pas nécessaire. Une note reliée à un seul élément est accrochée de son côté dans SIMP (`note left of …`) et coupée à la largeur de sa boîte.

Un symbole d’interface relié une seule fois porte son nom sur son lien et reste au rang voisin de sa classe. La visibilité d’une extrémité d’association n’est affichée qu’avec un nom de rôle. Un classifieur générique (`IComparable<T>`) utilise la notation UML de PlantUML, si ses paramètres ne contiennent que des caractères sans effet ; sinon le nom reste échappé.

Dans les vues enrichies, le XML des éléments sélectionnés, des occurrences et du contexte de vue est conservé dans des commentaires JSON `simp2puml-source`/`simp2puml-context`. C’est une archive consultable, **pas une conversion sémantique ni un mécanisme d’import inverse**. Les propriétés supplémentaires détectées sont affichées en annotation avec avertissement.

L’archive peut contenir les mêmes informations sensibles que le fichier SIMP source ; traiter la sortie comme ce fichier. Les notes et noms sont échappés pour rester du texte, sans directives, liens ou images chargées.

En séquence, les occurrences multiples d’une même identité sont réunies avec avertissement. Sans coordonnées de message, l’ordre XML sert de repli signalé. Les fragments croisés deviennent des séparateurs, et ceux sans intervalle graphique des notes. Les activations sont ancrées aux messages ; celles sans message intermédiaire sont omises du dessin, celles imbriquées sur une même ligne de vie sont réunies, avec avertissement et archive XML. Les durées exactes ne sont pas restituées.

Le lecteur PlantUML accepte un sous-ensemble strict, avec une instruction par ligne et des membres de la forme `nom: Type` ou `operation(parametre: Type): Retour`. Les packages, réalisations, valeurs par défaut, stéréotypes arbitraires et préprocesseur ne sont pas pris en charge. Les directives graphiques reconnues sont ignorées avec avertissement ; les autres constructions non prises en charge sont refusées. Aucun fichier externe n’est chargé. Voir [l’exemple minimal](../examples/minimal.puml).

L’export SIMP reste expérimental et limité au modèle historique de classes ; il génère une grille. L’export Atelier et l’import PlantUML ne sont pas élargis à toutes les familles : une vue SIMP enrichie est explicitement refusée par `write_atelier`/`write_simp`, plutôt que de produire une sortie vide. Les flèches d’association lues depuis PlantUML perdent encore leur direction avec avertissement. Le lecteur PlantUML interne ne peut donc pas relire toutes les sorties enrichies ; utiliser PlantUML lui-même pour les vérifier.

`--strict` refuse les approximations et pertes détectées, y compris le recalcul de mise en page signalé par les vues enrichies. Un avertissement peut concerner plusieurs éléments ; leur nombre n’est pas un décompte des éléments perdus. Vérifier la sortie dans l’outil cible avant de remplacer un document de travail.
