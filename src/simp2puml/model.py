"""Modèle UML commun : l'héritage va de l'enfant vers le parent."""
from dataclasses import dataclass, field


class ConversionError(ValueError):
    """Entrée invalide, non représentable ou non prise en charge."""


@dataclass
class UMLParameter:
    name: str
    type: str | None = None
    multiplicity: str | None = None


@dataclass
class UMLAttribute:
    name: str
    type: str | None = None
    visibility: str | None = None
    is_static: bool = False
    multiplicity: str | None = None


@dataclass
class UMLOperation:
    name: str
    parameters: list[UMLParameter] = field(default_factory=list)
    return_type: str | None = None
    visibility: str | None = None
    is_static: bool = False
    is_abstract: bool = False
    multiplicity: str | None = None


@dataclass
class UMLClass:
    id: str
    name: str
    attributes: list[UMLAttribute] = field(default_factory=list)
    operations: list[UMLOperation] = field(default_factory=list)
    kind: str = 'class'
    is_abstract: bool = False
    literals: list[str] = field(default_factory=list)


@dataclass
class UMLRelationship:
    source: str
    target: str
    kind: str = 'association'
    name: str | None = None
    source_multiplicity: str | None = None
    target_multiplicity: str | None = None
    source_role: str | None = None
    target_role: str | None = None
    source_visibility: str | None = None
    target_visibility: str | None = None


@dataclass
class UMLNote:
    id: str
    text: str
    targets: list[str] = field(default_factory=list)


@dataclass
class UMLDiagram:
    name: str = 'Diagram'
    classes: list[UMLClass] = field(default_factory=list)
    relationships: list[UMLRelationship] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    notes: list[UMLNote] = field(default_factory=list)

    def validate(self) -> None:
        ids: set[str] = set()
        for cls in self.classes:
            if not cls.id or cls.id in ids:
                raise ConversionError(f'Identifiant de classe absent ou dupliqué : {cls.id!r}')
            ids.add(cls.id)
            if not cls.name:
                raise ConversionError(f'Classe {cls.id} : nom vide')
            if cls.kind not in ('class', 'interface', 'enumeration', 'interface_symbol'):
                raise ConversionError(f'{cls.name} : nature de classifieur inconnue {cls.kind}')
            if cls.kind != 'enumeration' and cls.literals:
                raise ConversionError(f'{cls.name} : valeurs réservées aux énumérations')
            if any(not value for value in cls.literals):
                raise ConversionError(f'{cls.name} : valeur d’énumération vide')
            if cls.kind == 'interface_symbol' and (cls.attributes or cls.operations or cls.literals or cls.is_abstract):
                raise ConversionError(f'{cls.name} : symbole d’interface avec membres ou modificateurs')
            for member in [*cls.attributes, *cls.operations]:
                if not member.name:
                    raise ConversionError(f'Classe {cls.name} : membre sans nom')
                if member.visibility not in (None, '+', '-', '#', '~'):
                    raise ConversionError(f'{cls.name}.{member.name} : visibilité inconnue')
                if isinstance(member, UMLOperation):
                    if any(not p.name for p in member.parameters):
                        raise ConversionError(f'{cls.name}.{member.name} : paramètre sans nom')
        for relation in self.relationships:
            if relation.kind not in ('association', 'inheritance', 'aggregation', 'composition', 'provided_interface'):
                raise ConversionError(f'Type de relation inconnu : {relation.kind}')
            if relation.source not in ids or relation.target not in ids:
                raise ConversionError(f'Référence de relation introuvable : {relation.source} → {relation.target}')
            if relation.source_visibility not in (None, '+', '-', '#', '~') or relation.target_visibility not in (None, '+', '-', '#', '~'):
                raise ConversionError('Visibilité d’extrémité de relation inconnue')
            if relation.kind == 'provided_interface':
                target = next(cls for cls in self.classes if cls.id == relation.target)
                if target.kind != 'interface_symbol':
                    raise ConversionError('Un connecteur fourni doit cibler un symbole d’interface distinct')
        note_ids: set[str] = set()
        for note in self.notes:
            if not note.id or note.id in ids or note.id in note_ids:
                raise ConversionError(f'Identifiant de note absent ou dupliqué : {note.id!r}')
            note_ids.add(note.id)
            if any(target not in ids for target in note.targets):
                raise ConversionError(f'Note {note.id} : cible introuvable')
