//! Modèle : propositions d'autocomplétion PlantUML.
//!
//! Sources : mots-clés adaptés au contexte (début de ligne, directive `@`,
//! diagramme d'activité…), noms déjà déclarés dans le document, flèches et
//! extraits (snippets). Le classement suit le principe du « fuzzy matcher »
//! de Zed : préfixe exact d'abord, puis correspondance par sous-séquence,
//! en favorisant les débuts de mot et les caractères contigus.

use std::collections::BTreeSet;
use std::ops::Range;

/// Marqueur de position du curseur dans un extrait.
pub const CURSOR: &str = "$0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletionKind {
    // ordre de départage à score égal : le mot-clé avant son extrait
    Keyword,
    Snippet,
    Directive,
    Arrow,
    Name,
}

impl CompletionKind {
    pub fn label(self) -> &'static str {
        match self {
            CompletionKind::Snippet => "extrait",
            CompletionKind::Keyword => "mot-clé",
            CompletionKind::Name => "nom",
            CompletionKind::Arrow => "flèche",
            CompletionKind::Directive => "directive",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Completion {
    /// Texte affiché dans la liste.
    pub label: String,
    /// Texte inséré ; `$0` indique la position finale du curseur.
    pub insert: String,
    pub kind: CompletionKind,
    pub detail: &'static str,
}

/// Ensemble de propositions pour une position donnée.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletionList {
    /// Plage (octets) remplacée par la proposition acceptée.
    pub range: Range<usize>,
    pub items: Vec<Completion>,
}

const DECLARATIONS: &[(&str, &str)] = &[
    ("class", "classe"), ("abstract class", "classe abstraite"), ("interface", "interface"),
    ("enum", "énumération"), ("annotation", "annotation"), ("object", "objet"), ("map", "table"),
    ("package", "paquet"), ("namespace", "espace de noms"), ("participant", "participant"),
    ("actor", "acteur"), ("boundary", "frontière"), ("control", "contrôle"), ("entity", "entité"),
    ("database", "base de données"), ("collections", "collection"), ("queue", "file"),
    ("usecase", "cas d'utilisation"), ("component", "composant"), ("node", "nœud"),
    ("cloud", "nuage"), ("artifact", "artefact"), ("folder", "dossier"), ("frame", "cadre"),
    ("rectangle", "rectangle"), ("card", "carte"), ("storage", "stockage"), ("state", "état"),
    ("note", "note"), ("title", "titre"), ("legend", "légende"), ("header", "en-tête"),
    ("footer", "pied de page"), ("caption", "légende de figure"), ("skinparam", "style (ignoré par l'aperçu)"),
    ("hide", "masquer"), ("show", "afficher"), ("left to right direction", "orientation"),
    ("top to bottom direction", "orientation"), ("together", "regroupement"),
];

const SEQUENCE: &[(&str, &str)] = &[
    ("activate", "activation"), ("deactivate", "fin d'activation"), ("destroy", "destruction"),
    ("create", "création"), ("autonumber", "numérotation"), ("alt", "alternative"), ("else", "sinon"),
    ("opt", "option"), ("loop", "boucle"), ("par", "parallèle"), ("break", "interruption"),
    ("critical", "région critique"), ("group", "groupe"), ("end", "fin de fragment"),
    ("ref over", "référence"), ("note over", "note"), ("return", "retour"), ("box", "boîte"),
    ("end box", "fin de boîte"), ("newpage", "nouvelle page"),
];

const ACTIVITY: &[(&str, &str)] = &[
    ("start", "début"), ("stop", "fin"), ("end", "fin"), ("if", "condition"), ("elseif", "sinon si"),
    ("else", "sinon"), ("endif", "fin de condition"), ("while", "boucle"), ("endwhile", "fin de boucle"),
    ("repeat", "répétition"), ("repeat while", "condition de répétition"), ("fork", "parallélisme"),
    ("fork again", "branche parallèle"), ("end fork", "fin du parallélisme"), ("switch", "aiguillage"),
    ("case", "cas"), ("endswitch", "fin d'aiguillage"), ("partition", "partition"), ("detach", "détacher"),
    ("kill", "terminer"),
];

const DIRECTIVES: &[(&str, &str)] = &[
    ("@startuml", "début de diagramme"), ("@enduml", "fin de diagramme"),
    ("@startmindmap", "carte mentale"), ("@endmindmap", "fin de carte mentale"),
    ("@startwbs", "organigramme de tâches"), ("@endwbs", "fin de WBS"),
    ("@startgantt", "Gantt"), ("@endgantt", "fin de Gantt"),
    ("@startjson", "données JSON"), ("@endjson", "fin de JSON"),
    ("@startyaml", "données YAML"), ("@endyaml", "fin de YAML"),
];

const ARROWS: &[(&str, &str)] = &[
    ("-->", "association / transition"), ("->", "message / lien horizontal"), ("..>", "dépendance"),
    ("--|>", "héritage"), ("..|>", "réalisation"), ("--*", "composition"), ("--o", "agrégation"),
    ("<|--", "héritage (parent à gauche)"), ("*--", "composition (tout à gauche)"),
    ("o--", "agrégation (tout à gauche)"), ("--", "lien"), ("..", "lien pointillé / note"),
    ("-->>", "message asynchrone"), ("<--", "retour"), ("-up->", "flèche vers le haut"),
    ("-left->", "flèche vers la gauche"), ("-right->", "flèche vers la droite"),
];

const SNIPPETS: &[(&str, &str, &str)] = &[
    ("class … { }", "class $0 {\n}", "classe avec corps"),
    ("interface … { }", "interface $0 {\n}", "interface avec corps"),
    ("enum … { }", "enum $0 {\n}", "énumération avec corps"),
    ("package … { }", "package \"$0\" {\n}", "paquet"),
    ("note right of …", "note right of $0\nend note", "note multiligne"),
    ("alt … else … end", "alt $0\nelse\nend", "fragment alternatif"),
    ("loop … end", "loop $0\nend", "boucle"),
    ("if … then … endif", "if ($0) then (oui)\nelse (non)\nendif", "condition d'activité"),
    ("while … endwhile", "while ($0) is (oui)\nendwhile (non)", "boucle d'activité"),
    ("fork … end fork", "fork\n  $0\nfork again\nend fork", "parallélisme"),
    ("state … { }", "state $0 {\n}", "état composite"),
    ("legend … endlegend", "legend right\n  $0\nendlegend", "légende"),
];

const NAME_KEYWORDS: &[&str] = &[
    "class", "interface", "enum", "annotation", "object", "map", "participant", "actor", "boundary",
    "control", "entity", "database", "collections", "queue", "usecase", "component", "node", "cloud",
    "artifact", "folder", "frame", "rectangle", "card", "storage", "state", "package", "namespace",
    "abstract", "protocol", "struct", "agent", "person", "hexagon", "label", "file", "stack",
];

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '@'
}

/// Noms déclarés ou utilisés dans le document (classes, participants, alias…).
pub fn document_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut add = |s: &str| {
        let s = s.trim().trim_matches('"').trim_matches(|c| c == '[' || c == ']' || c == '(' || c == ')' || c == ':');
        if !s.is_empty() && s.len() < 80 && s.chars().all(|c| c.is_alphanumeric() || "_.$ ".contains(c)) && !s.contains("  ") {
            names.insert(s.to_string());
        }
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('\'') || line.starts_with('@') || line.starts_with('!') {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(first) = words.first() else { continue };
        let first_lower = first.to_lowercase();
        if NAME_KEYWORDS.contains(&first_lower.as_str()) {
            let rest = &words[1..];
            let rest = if first_lower == "abstract" && rest.first() == Some(&"class") { &rest[1..] } else { rest };
            if let Some(name) = rest.first() {
                if name.starts_with('"') {
                    // « "Nom long" as Alias » : on garde l'alias
                } else {
                    add(name.trim_end_matches('{'));
                }
            }
            if let Some(i) = rest.iter().position(|w| *w == "as") {
                if let Some(alias) = rest.get(i + 1) {
                    add(alias.trim_end_matches('{'));
                }
            }
            continue;
        }
        // relations : « A --> B : texte » (avant les deux-points)
        let head = line.split(':').next().unwrap_or("");
        if head.contains("--") || head.contains("->") || head.contains("..") || head.contains("<-") {
            let ends: Vec<&str> = head
                .split(|c: char| "-.<>|*".contains(c))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            if let (Some(a), Some(b)) = (ends.first(), ends.last()) {
                add(a.split_whitespace().next().unwrap_or(""));
                add(b.split_whitespace().last().unwrap_or(""));
            }
        }
    }
    names
}

/// Score de correspondance approximative ; `None` si `query` n'est pas une
/// sous-séquence de `candidate` (insensible à la casse).
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    // une minuscule par caractère : `to_lowercase` d'une chaîne peut changer
    // le nombre de caractères (« İ » en donne deux) et décaler les indices
    let fold = |ch: char| ch.to_lowercase().next().unwrap_or(ch);
    let q: Vec<char> = query.chars().map(fold).collect();
    let c: Vec<char> = candidate.chars().collect();
    let cl: Vec<char> = c.iter().copied().map(fold).collect();
    if cl.starts_with(&q) {
        // préfixe : meilleur score, les candidats courts d'abord
        return Some(1000 - c.len() as i32 + if c.starts_with(&query.chars().collect::<Vec<_>>()) { 10 } else { 0 });
    }
    let mut score = 0;
    let mut qi = 0;
    let mut prev: Option<usize> = None;
    for (i, ch) in cl.iter().enumerate() {
        if qi < q.len() && *ch == q[qi] {
            let word_start = i == 0 || !c[i - 1].is_alphanumeric() || (c[i].is_uppercase() && c[i - 1].is_lowercase());
            score += if word_start { 30 } else { 5 };
            if prev == Some(i.wrapping_sub(1)) {
                score += 15; // caractères contigus
            }
            prev = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then(|| score - c.len() as i32)
}

/// Contexte grossier du document, pour proposer les bons mots-clés : séquence, activité.
fn diagram_flavour(text: &str) -> (bool, bool) {
    let mut sequence = false;
    let mut activity = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with(':') && t.ends_with(';') || t == "start" || t.starts_with("if (") {
            activity = true;
        }
        let lw = t.to_lowercase();
        if ["participant ", "activate ", "alt ", "loop ", "autonumber"].iter().any(|k| lw.starts_with(k)) {
            sequence = true;
        }
        if t.contains("->") && !t.contains("--") {
            sequence = true;
        }
    }
    (sequence, activity)
}

/// Noms et contexte de tout le document sauf la ligne en cours. Noms et contexte se lisent ligne
/// par ligne : en tapant dans une même ligne, le reste n'est pas relu (il est seulement comparé,
/// bien moins coûteux), et l'autocomplétion ne ralentit plus la frappe sur un gros document.
#[derive(Default)]
pub struct DocumentIndex {
    known: Option<(String, String)>,
    names: BTreeSet<String>,
    flavour: (bool, bool),
}

impl DocumentIndex {
    fn update(&mut self, before: &str, after: &str) {
        if self.known.as_ref().is_some_and(|(b, a)| b == before && a == after) {
            return;
        }
        self.names = document_names(before);
        self.names.extend(document_names(after));
        let (first, second) = (diagram_flavour(before), diagram_flavour(after));
        self.flavour = (first.0 || second.0, first.1 || second.1);
        self.known = Some((before.to_string(), after.to_string()));
    }
}

/// Propositions pour le curseur `cursor` ; `explicit` = demandé par Ctrl+Espace
/// (propose même sans préfixe).
pub fn complete(text: &str, cursor: usize, explicit: bool, index: &mut DocumentIndex) -> Option<CompletionList> {
    let line_start = text[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let before = &text[line_start..cursor];
    let trimmed = before.trim_start();
    let at_line_start = trimmed.chars().all(is_word) || trimmed.is_empty();

    // préfixe d'une flèche : « A -- » ou « A .. »
    let arrow_start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| "-.<>|*o".contains(*c))
        .last()
        .map(|(i, _)| i);
    if let Some(a) = arrow_start {
        let prefix = &before[a..];
        let preceded_by_space = before[..a].ends_with(' ');
        if prefix.len() >= 2 && preceded_by_space && (prefix.starts_with('-') || prefix.starts_with('.') || prefix.starts_with('<')) {
            let items = rank(prefix, ARROWS.iter().map(|(a, d)| Completion {
                label: a.to_string(),
                insert: format!("{a} "),
                kind: CompletionKind::Arrow,
                detail: d,
            }));
            return (!items.is_empty()).then(|| CompletionList { range: line_start + a..cursor, items });
        }
    }

    // mot en cours (lettres, chiffres, _ et @ en début)
    let word_start = before.char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map(|(i, _)| i).unwrap_or(before.len());
    let prefix = &before[word_start..];
    if prefix.is_empty() && !explicit {
        return None;
    }
    let range = line_start + word_start..cursor;
    let mut candidates: Vec<Completion> = Vec::new();
    let line_end = text[cursor..].find('\n').map_or(text.len(), |i| cursor + i);
    let line = &text[line_start..line_end];
    let word = |label: &str, kind, detail| Completion { label: label.to_string(), insert: label.to_string(), kind, detail };

    if prefix.starts_with('@') || (at_line_start && trimmed.starts_with('@')) {
        candidates.extend(DIRECTIVES.iter().map(|(k, d)| word(k, CompletionKind::Directive, d)));
    } else {
        let first_word = at_line_start && !before[..word_start].trim().contains(' ');
        if first_word {
            index.update(&text[..line_start], &text[line_end..]);
            let here = diagram_flavour(line);
            let (sequence, activity) = (index.flavour.0 || here.0, index.flavour.1 || here.1);
            // les déclarations attendent un nom : on insère l'espace qui suit
            candidates.extend(DECLARATIONS.iter().map(|(k, d)| {
                let standalone = k.ends_with("direction") || *k == "together" || *k == "legend";
                Completion {
                    label: k.to_string(),
                    insert: if standalone { k.to_string() } else { format!("{k} ") },
                    kind: CompletionKind::Keyword,
                    detail: d,
                }
            }));
            if sequence {
                candidates.extend(SEQUENCE.iter().map(|(k, d)| word(k, CompletionKind::Keyword, d)));
            }
            if activity {
                candidates.extend(ACTIVITY.iter().map(|(k, d)| word(k, CompletionKind::Keyword, d)));
            }
            candidates.extend(SNIPPETS.iter().map(|(label, insert, d)| Completion {
                label: label.to_string(),
                insert: insert.to_string(),
                kind: CompletionKind::Snippet,
                detail: d,
            }));
        } else {
            for (k, d) in [("as", "alias"), ("of", "cible de note"), ("extends", "héritage"), ("implements", "réalisation")] {
                candidates.push(word(k, CompletionKind::Keyword, d));
            }
        }
        // noms du document (hors mot en cours de frappe)
        index.update(&text[..line_start], &text[line_end..]);
        for name in index.names.iter().chain(&document_names(line)) {
            if name != prefix && !name.contains(' ') && fuzzy_score(prefix, name).is_some() {
                candidates.push(word(name, CompletionKind::Name, "déclaré dans le document"));
            }
        }
    }
    let items = rank(prefix, candidates.into_iter());
    // inutile de proposer exactement ce qui est déjà tapé
    if items.len() == 1 && items[0].insert == prefix {
        return None;
    }
    (!items.is_empty()).then_some(CompletionList { range, items })
}

fn rank(prefix: &str, candidates: impl Iterator<Item = Completion>) -> Vec<Completion> {
    // un extrait est classé sur son premier mot (« class … { } » comme « class »)
    let key = |c: &Completion| -> String {
        if c.kind == CompletionKind::Snippet { c.label.split(' ').next().unwrap_or("").to_string() } else { c.label.clone() }
    };
    let mut scored: Vec<(i32, String, Completion)> = candidates
        .filter_map(|c| {
            let k = key(&c);
            fuzzy_score(prefix, &k).map(|s| (s, k, c))
        })
        .collect();
    // score, puis mot (l'extrait suit son mot-clé), puis nature
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.kind.cmp(&b.2.kind)));
    let mut seen = BTreeSet::new();
    scored
        .into_iter()
        .filter(|(_, _, c)| seen.insert(c.label.clone()))
        .map(|(_, _, c)| c)
        .take(40)
        .collect()
}

/// Texte à insérer et position du curseur relative au début de l'insertion,
/// avec l'indentation de la ligne appliquée aux lignes suivantes de l'extrait.
pub fn expand(insert: &str, indent: &str) -> (String, usize) {
    let indented = insert.replace('\n', &format!("\n{indent}"));
    match indented.find(CURSOR) {
        Some(i) => (indented.replacen(CURSOR, "", 1), i),
        None => {
            let len = indented.len();
            (indented, len)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;

    fn complete(text: &str, cursor: usize, explicit: bool) -> Option<CompletionList> {
        super::complete(text, cursor, explicit, &mut DocumentIndex::default())
    }

    #[test]
    fn the_index_gives_the_same_list_as_reading_everything() {
        fn seen(l: Option<CompletionList>) -> Option<(Vec<String>, Range<usize>)> {
            l.map(|l| (l.items.iter().map(|c| c.insert.clone()).collect(), l.range))
        }
        let mut index = DocumentIndex::default();
        let mut text = String::from("@startuml\nclass Alpha\nparticipant Bob\nA -> Bob : x\n\nclass Delta\n@enduml\n");
        // frappe dans une ligne, puis une autre, puis une modification ailleurs dans le document
        let mut at = text.find("\n\nclass Delta").unwrap() + 1;
        for typed in ["D", "e", "l"] {
            text.insert_str(at, typed);
            at += typed.len();
            assert_eq!(seen(super::complete(&text, at, false, &mut index)), seen(complete(&text, at, false)), "{text}");
        }
        text.insert_str(at, "\nA");
        at += 2;
        assert_eq!(seen(super::complete(&text, at, false, &mut index)), seen(complete(&text, at, false)));
        text = text.replace("class Alpha", "class Zulu");
        at = text.find("\nA\n").unwrap() + 2;
        let list = super::complete(&text, at, true, &mut index);
        assert_eq!(seen(list), seen(complete(&text, at, true)), "une ligne changée ailleurs compte aussi");
        // le contexte dépend aussi de la ligne en cours
        let mut fresh = DocumentIndex::default();
        let seq = "@startuml\nparticipant A\nlo";
        assert!(super::complete(seq, seq.len(), false, &mut fresh).is_some_and(|l| l.items.iter().any(|c| c.label == "loop")));
    }

    #[test]
    fn case_folding_that_changes_length_does_not_panic() {
        assert!(fuzzy_score("ul", "İstanbul").is_some());
        assert!(fuzzy_score("is", "İstanbul").is_some());
        let text = "@startuml\nclass İstanbul\nul";
        assert!(complete(text, text.len(), false).is_some());
    }

    fn labels(l: &CompletionList) -> Vec<&str> {
        l.items.iter().map(|c| c.label.as_str()).collect()
    }

    #[test]
    fn keywords_at_line_start() {
        let text = "@startuml\ncla";
        let l = complete(text, text.len(), false).unwrap();
        assert_eq!(l.range, 10..13);
        assert_eq!(labels(&l)[0], "class");
        assert_eq!(labels(&l)[1], "class … { }");
        assert_eq!(l.items[0].insert, "class ");
    }

    #[test]
    fn directives_after_at() {
        let text = "@st";
        let l = complete(text, 3, false).unwrap();
        assert_eq!(l.range, 0..3);
        assert!(labels(&l).iter().all(|s| s.starts_with('@')));
        assert_eq!(labels(&l)[0], "@startuml");
    }

    #[test]
    fn document_names_are_proposed() {
        let text = "@startuml\nclass Commande\nparticipant \"Long\" as Serveur\nCommande --> Ligne\nSer";
        let names = document_names(text);
        assert!(names.contains("Commande") && names.contains("Serveur") && names.contains("Ligne"));
        let l = complete(text, text.len(), false).unwrap();
        assert_eq!(labels(&l)[0], "Serveur");
    }

    #[test]
    fn names_after_an_arrow() {
        let text = "class Commande\nclass Client\nClient --> Com";
        let l = complete(text, text.len(), false).unwrap();
        assert_eq!(labels(&l)[0], "Commande");
        assert!(!labels(&l).contains(&"class"));
    }

    #[test]
    fn arrows_are_completed() {
        let text = "A --|";
        let l = complete(text, text.len(), false).unwrap();
        assert_eq!(l.range, 2..5);
        assert_eq!(labels(&l)[0], "--|>");
    }

    #[test]
    fn activity_keywords_only_in_activity_diagrams() {
        let seq = "@startuml\nA -> B\nlo";
        assert!(labels(&complete(seq, seq.len(), false).unwrap()).contains(&"loop"));
        let act = "@startuml\nstart\n:x;\nendw";
        assert_eq!(labels(&complete(act, act.len(), false).unwrap())[0], "endwhile");
    }

    #[test]
    fn fuzzy_prefers_prefix_then_word_starts() {
        assert!(fuzzy_score("par", "participant").unwrap() > fuzzy_score("par", "partition").unwrap() - 100);
        assert!(fuzzy_score("ldr", "left to right direction").is_some());
        assert!(fuzzy_score("xyz", "class").is_none());
        assert!(fuzzy_score("cl", "class").unwrap() > fuzzy_score("cl", "collections").unwrap());
    }

    #[test]
    fn nothing_without_prefix_unless_explicit() {
        let text = "@startuml\n";
        assert!(complete(text, text.len(), false).is_none());
        assert!(complete(text, text.len(), true).is_some());
    }

    #[test]
    fn snippet_expansion_keeps_indent_and_cursor() {
        let (s, c) = expand("class $0 {\n}", "  ");
        assert_eq!(s, "class  {\n  }");
        assert_eq!(c, 6);
    }
}
