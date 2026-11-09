# Installation

Le dépôt fournit deux choses : le convertisseur avec son interface classique (Python), et l'éditeur simp2puml (Rust et Nim). `simp2puml-gui` ouvre l'éditeur s'il le trouve, l'interface classique sinon.

## Télécharger

Chaque version publiée est compilée pour les trois systèmes. L'archive contient le projet de cette version, l'éditeur et son moteur de rendu déjà construits :

| Système | Archive | Lanceur |
| --- | --- | --- |
| Windows (x64) | [simp2puml-windows-x64.zip](https://github.com/Seven0492/simp2puml/releases/latest/download/simp2puml-windows-x64.zip) | `simp2puml-windows.cmd` |
| macOS (Apple Silicon et Intel) | [simp2puml-macos.zip](https://github.com/Seven0492/simp2puml/releases/latest/download/simp2puml-macos.zip) | `simp2puml-macos.command` |
| Linux (x64, glibc 2.35 ou plus récente) | [simp2puml-linux-x64.tar.gz](https://github.com/Seven0492/simp2puml/releases/latest/download/simp2puml-linux-x64.tar.gz) | `simp2puml-linux.sh` |

Extraire l'archive, puis ouvrir le lanceur comme décrit dans [Démarrer en un double-clic](#demarrer-en-un-double-clic) : il ouvre directement l'éditeur précompilé. Les binaires ne sont pas signés par un éditeur reconnu : Windows demande de confirmer, et macOS demande le clic droit → Ouvrir sur le lanceur. Le lanceur macOS lève ensuite la mise en quarantaine des deux binaires de l'éditeur, et d'eux seuls. Les [notes de version](https://github.com/Seven0492/simp2puml/releases/latest) donnent les sommes SHA-256 ; chaque release est aussi recopiée sur [Forgejo](https://git.sethmartin.dev/Seth_Martin/simp2puml/releases).

Les archives sont construites automatiquement à chaque tag de version (voir [Publier une version](developpement.md#publier-une-version)). Les versions macOS et Linux n'ont pas encore été essayées sur un poste réel.

## L'éditeur simp2puml

Construire (Nim ≥ 1.6, Rust stable ; sous Linux, GPUI requiert notamment les en-têtes `libxkbcommon`, `wayland`, `vulkan`, `fontconfig` et `xcb`) :

```sh
cd renderer && nimble build        # ou : nim c -o:bin/pumlrender src/pumlrender.nim
cd ../editor && cargo build --release
./target/release/puml-editor diagramme.puml
```

Sous Linux, l'entrée de menu `editor/simp2puml.desktop` s'installe avec le binaire et le moteur placés côte à côte :

```sh
install -Dm755 editor/target/release/puml-editor ~/.local/bin/puml-editor
install -Dm755 renderer/bin/pumlrender ~/.local/bin/pumlrender
install -Dm644 src/simp2puml/assets/icon-256.png ~/.local/share/icons/hicolor/256x256/apps/simp2puml.png
install -Dm644 editor/simp2puml.desktop ~/.local/share/applications/simp2puml.desktop
update-desktop-database ~/.local/share/applications
```

Les deux interfaces partagent le même fichier de préférences. Pour en changer, voir [Préférences](editeur.md#preferences).

## Démarrer en un double-clic

Pour utiliser l’interface graphique sans connaître Python, télécharger le projet (archive ZIP), l’extraire, puis ouvrir le lanceur de son système :

| Système | Lanceur | Premier lancement |
| --- | --- | --- |
| Windows | `simp2puml-windows.cmd` | Double-clic. Si Windows affiche un avertissement (fichier téléchargé), choisir « Exécuter ». Raccourcis « simp2puml » créés dans le menu Démarrer et sur le Bureau. |
| macOS | `simp2puml-macos.command` | Clic droit → Ouvrir la première fois (Gatekeeper bloque le double-clic sur un script téléchargé). L’application « simp2puml » est ensuite ajoutée à `~/Applications` (Launchpad, Spotlight). |
| Linux | `simp2puml-linux.sh` | « Exécuter » depuis le gestionnaire de fichiers, ou `sh simp2puml-linux.sh`. Une entrée « simp2puml » est ajoutée au menu des applications. |

Le lanceur utilise un Python ≥ 3.11 avec Tkinter s’il en trouve un. Sinon, il télécharge [uv](https://docs.astral.sh/uv/) (version fixée ; son installateur vérifie les sommes SHA-256), qui installe un Python 3.12 privé, **sans droits administrateur et sans toucher au Python du système**. Tout est rangé dans un seul dossier : `%LOCALAPPDATA%\simp2puml` (Windows), `~/Library/Application Support/simp2puml` (macOS) ou `~/.local/share/simp2puml` (Linux).

Désinstaller revient à supprimer ce dossier et le raccourci créé. Une connexion Internet n’est nécessaire qu’au premier lancement. Les raccourcis pointent vers le dossier extrait : relancer le lanceur après l’avoir déplacé.

Sous Windows, le premier lancement passe par PowerShell ; les suivants n’en ont plus besoin : `simp2puml-windows.cmd` lit le Python mémorisé (`pythonw-cmd.txt` dans le dossier de données) et ouvre l’interface directement, en ≈ 0,15 s au lieu de ≈ 0,5 s, sans fenêtre de console qui reste ouverte. Si ce Python a disparu, ou si son chemin contient des caractères non ASCII, le lanceur repasse par PowerShell. Les raccourcis du menu Démarrer et du Bureau lancent aussi l’interface sans PowerShell.

Le lanceur Windows a été vérifié sur Windows 11, avec et sans Python préinstallé. Les lanceurs macOS et Linux sont vérifiés par ShellCheck, mais n’ont pas encore été exécutés sur ces systèmes.

## Installer le convertisseur avec pipx

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
