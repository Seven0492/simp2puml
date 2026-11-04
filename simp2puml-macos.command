#!/bin/sh
# Double-cliquez dans le Finder pour lancer simp2puml. Installe Python au besoin, sans droits administrateur.
exec /bin/sh "$(dirname -- "$0")/launchers/bootstrap.sh" "$@"
