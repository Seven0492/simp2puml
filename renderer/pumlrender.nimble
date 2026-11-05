# Paquet Nim du moteur de rendu natif de l'éditeur PlantUML.
version       = "0.1.0"
author        = "Nela"
description   = "Rendu SVG natif de diagrammes PlantUML (sans Java ni préprocesseur)"
license       = "MIT"
srcDir        = "src"
binDir        = "bin"
bin           = @["pumlrender"]

requires "nim >= 1.6.0"

task test, "Lance les tests du moteur de rendu":
  exec "nim c -r --hints:off --path:src tests/test_render.nim"
