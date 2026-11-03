Import de simp2puml

Projet original : ASaki (Nela)
https://github.com/2534296-Mailloux-Nela/simp2puml
Licence MIT et copyright de Nela conservés dans LICENSE.

Les fichiers source correspondent au commit local :
0a1fdab28dccac9786a28450d97c8a3b53d67345

Le portail Forgejo importe les fichiers en plusieurs commits. L'historique Git
complet d'origine et des améliorations est conservé séparément dans :
provenance/simp2puml-history.bundle

Pour restaurer cet historique et les permissions originales, depuis un clone
de ce dépôt :
  git clone -b improve-conversion-reliability provenance/simp2puml-history.bundle ../simp2puml-historique

Les fichiers créés par l'import texte sont de mode 100644. Pour utiliser
directement les règles Debian depuis ce snapshot, restaurer au besoin :
  chmod +x packaging/debian/rules
L'outil tools/build-native.py rétablit déjà ce bit lors de sa préparation Debian.

Validation du code importé : 105 tests réussis ; 2 tests avec la fixture privée
Monopoly.simp ignorés en son absence. Construction et installation isolée de la
wheel, aller-retour du modèle entre formats et protection des sorties vérifiés.
La fixture privée et les exports personnels ne sont pas inclus.
