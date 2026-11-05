//! Modèle : modèles de nouveaux diagrammes proposés par l'éditeur.

pub struct Template {
    pub name: &'static str,
    pub text: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        name: "Classes",
        text: "@startuml\ntitle Diagramme de classes\n\nabstract class Forme {\n  # nom : String\n  + {abstract} aire() : double\n}\n\nclass Cercle {\n  - rayon : double\n  + aire() : double\n}\n\ninterface Dessinable {\n  + dessiner()\n}\n\nForme <|-- Cercle\nCercle ..|> Dessinable\n@enduml\n",
    },
    Template {
        name: "Séquence",
        text: "@startuml\nactor Utilisateur\nparticipant Application as App\ndatabase Base\n\nUtilisateur -> App : connexion\nactivate App\nApp -> Base : vérifier\nBase --> App : ok\nApp --> Utilisateur : bienvenue\ndeactivate App\n@enduml\n",
    },
    Template {
        name: "Activité",
        text: "@startuml\nstart\n:Lire la demande;\nif (valide ?) then (oui)\n  :Traiter;\nelse (non)\n  :Rejeter;\nendif\nstop\n@enduml\n",
    },
    Template {
        name: "États",
        text: "@startuml\n[*] --> Brouillon\nBrouillon --> Publié : publier\nPublié --> Archivé : archiver\nArchivé --> [*]\n@enduml\n",
    },
    Template {
        name: "Cas d'utilisation",
        text: "@startuml\nleft to right direction\nactor Client\nrectangle Système {\n  (Commander)\n  (Payer)\n}\nClient --> (Commander)\nClient --> (Payer)\n@enduml\n",
    },
    Template {
        name: "Composants",
        text: "@startuml\npackage \"Application\" {\n  [Interface] as UI\n  [Service] as S\n}\ndatabase \"Données\" as DB\nUI --> S\nS --> DB\n@enduml\n",
    },
    Template {
        name: "Carte mentale",
        text: "@startmindmap\n* Idée\n** Branche A\n*** Détail\n** Branche B\n-- Branche gauche\n@endmindmap\n",
    },
    Template {
        name: "WBS",
        text: "@startwbs\n* Projet\n** Lot 1\n*** Tâche 1.1\n** Lot 2\n@endwbs\n",
    },
    Template {
        name: "Gantt",
        text: "@startgantt\nProject starts 2026-01-05\n[Analyse] lasts 5 days\nthen [Réalisation] lasts 10 days\n[Recette] lasts 3 days and starts at [Réalisation]'s end\n@endgantt\n",
    },
    Template {
        name: "JSON",
        text: "@startjson\n{\n  \"nom\": \"exemple\",\n  \"valeurs\": [1, 2, 3]\n}\n@endjson\n",
    },
];

pub fn blank() -> &'static str {
    "@startuml\n\n@enduml\n"
}
