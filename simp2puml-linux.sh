#!/bin/sh
# Lancez ce fichier (double-clic « Exécuter », ou « sh simp2puml-linux.sh ») une première fois ;
# simp2puml apparaît ensuite dans le menu des applications. Installe Python au besoin, sans droits administrateur.
exec /bin/sh "$(dirname -- "$0")/launchers/bootstrap.sh" "$@"
