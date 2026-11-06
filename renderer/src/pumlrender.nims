# Sans ces options, « nim c src/pumlrender.nim » produit un moteur 4 fois plus lent.
# Pas de -d:danger : le texte analysé vient de l'utilisateur, les contrôles de bornes restent.
switch("define", "release")
switch("opt", "speed")
switch("passC", "-flto")
switch("passL", "-flto")
