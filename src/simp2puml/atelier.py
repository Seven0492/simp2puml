"""Export vers le DSL 2.0 d'Atelier Diagrammes (blocs Typst ``diagramme-ad``).

Écriture seule : le modèle commun est traduit en source ``.atelier`` que le
package Typst ``atelier-diagrammes`` 2.x analyse, place et dessine. Les
identités du modèle deviennent des identifiants ASCII uniques ; le nom affiché
est conservé comme titre cité lorsqu'il diffère. Les références de relations et
de notes sont toujours absolues (``::Nom``) pour éviter toute résolution par
portée ou collision avec un mot-clé contextuel.

Les symboles d'interface deviennent des classifieurs ``circle`` reliés par une
association à leur fournisseur. Les notes sans cible exigent une position
dans le DSL : elles sont empilées à gauche. Les pertes détectées (multiplicités
non représentables, visibilités d'extrémité, positions arbitraires) sont
ajoutées à ``UMLDiagram.warnings``.
"""
import json
import re
import unicodedata

from .model import ConversionError, UMLAttribute, UMLDiagram, UMLOperation

_IDENT = re.compile(r'[A-Za-z_][A-Za-z0-9_]*\Z')
# Mots-clés contextuels du DSL 2 : évités comme identifiants par prudence,
# même si les références absolues les désambiguïsent.
_KEYWORDS = frozenset({
    'dsl', 'package', 'namespace', 'sequence', 'class', 'interface', 'enum', 'struct',
    'record', 'dataclass', 'annotation', 'entity', 'exception', 'metaclass', 'protocol',
    'stereotype', 'primitive', 'circle', 'diamond', 'fn', 'compartment', 'field', 'method',
    'member', 'static', 'abstract', 'note', 'on', 'with', 'lollipop', 'content', 'json',
    'align', 'order', 'together', 'layout', 'view', 'views', 'rich', 'true', 'false',
})
_MULTIPLICITY = re.compile(r'(?:\*|(\d+)(?:\.\.(\d+|\*))?)\Z')
_OPENERS = {'(': ')', '[': ']', '<': '>'}
_MAX_TEXT = 2048
_MAX_MEMBERS = 200
_FREE_NOTE_STEP = 48


def _string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def _balanced(value: str) -> bool:
    stack = []
    for char in value:
        if char in _OPENERS:
            stack.append(_OPENERS[char])
        elif char in _OPENERS.values():
            if not stack or stack.pop() != char:
                return False
    return not stack


def _text(value: str, what: str) -> str:
    """Texte opaque, cité uniquement lorsque la forme brute serait ambiguë."""
    if len(value.encode('utf-8')) > _MAX_TEXT:
        raise ConversionError(f'{what} : texte supérieur à {_MAX_TEXT} octets')
    if (not value or value != value.strip() or value.startswith(('"', 'rich"', '//'))
            or any(c in value for c in '{}"\\') or any(ord(c) < 32 or ord(c) == 127 for c in value)
            or not _balanced(value)):
        return _string(value)
    return value


def _identifiers(diagram: UMLDiagram) -> dict[str, str]:
    """Identifiants DSL stables et uniques, dérivés des noms affichés."""
    result: dict[str, str] = {}
    used: set[str] = set()
    for cls in diagram.classes:
        ascii_name = unicodedata.normalize('NFKD', cls.name).encode('ascii', 'ignore').decode()
        base = re.sub(r'[^A-Za-z0-9_]+', '_', ascii_name).strip('_') or 'Element'
        if not _IDENT.fullmatch(base):
            base = 'C_' + base
        if base in _KEYWORDS or base == '__variables':
            base += '_'
        candidate, n = base, 2
        while candidate in used:
            candidate, n = f'{base}_{n}', n + 1
        used.add(candidate)
        result[cls.id] = candidate
    return result


def _multiplicity(value: str | None, context: str, warnings: set[str]) -> str | None:
    if value is None:
        return None
    match = _MULTIPLICITY.fullmatch(value)
    if match and (match[2] in (None, '*') or int(match[1]) <= int(match[2])):
        return value
    warnings.add(f'Multiplicité « {value} » non représentable en DSL Atelier, omise ({context})')
    return None


def _member_suffix(typ: str | None, mult: str | None) -> str:
    return (' : ' + typ if typ is not None else '') + (' [' + mult + ']' if mult is not None else '')


def _member_line(member: UMLAttribute | UMLOperation, owner: str) -> str:
    lead = ['field' if isinstance(member, UMLAttribute) else 'method']
    if member.visibility is not None:
        lead.append(member.visibility)
    if member.is_static:
        lead.append('static')
    if isinstance(member, UMLOperation):
        if member.is_abstract:
            lead.append('abstract')
        params = ', '.join(p.name + _member_suffix(p.type, p.multiplicity) for p in member.parameters)
        signature = f'{member.name}({params}){_member_suffix(member.return_type, member.multiplicity)}'
    else:
        signature = member.name + _member_suffix(member.type, member.multiplicity)
    return ' '.join(lead) + ' ' + _text(signature, f'{owner}.{member.name}')


def write_atelier(diagram: UMLDiagram) -> str:
    """Génère une source DSL 2.0 ; les pertes détectées enrichissent ``diagram.warnings``."""
    if diagram.source_view is not None:
        raise ConversionError('Cette vue SIMP enrichie exige --to puml ; export Atelier non pris en charge')
    diagram.validate()
    ids = _identifiers(diagram)
    by_id = {cls.id: cls for cls in diagram.classes}
    warnings: set[str] = set()
    lines = ['dsl 2']
    if diagram.name != 'Diagram':
        lines.append('// ' + ' '.join(diagram.name.split()))

    for cls in diagram.classes:
        ident = ids[cls.id]
        members = [*cls.attributes, *cls.operations]
        if len(members) + len(cls.literals) > _MAX_MEMBERS:
            raise ConversionError(f'{cls.name} : plus de {_MAX_MEMBERS} membres')
        title = f' {_string(cls.name)}' if cls.name != ident else ''
        prefix = 'abstract ' if cls.is_abstract else ''
        if cls.kind == 'interface_symbol':
            lines.append(f'circle {ident}{title}')
            continue
        if cls.kind == 'enumeration' and not members:
            keyword, stereotype = 'enum', ''
        elif cls.kind == 'enumeration':
            # Un corps enum n'admet que des littéraux : classe stéréotypée.
            keyword, stereotype = 'class', ' <<enumeration>>'
        else:
            keyword, stereotype = cls.kind, ''
        header = f'{prefix}{keyword} {ident}{title}{stereotype}'
        if not members and not cls.literals:
            lines.append(header)
            continue
        lines.append(header + ' {')
        if keyword == 'enum':
            lines.extend('  ' + _text(value, f'{cls.name} (valeur)') for value in cls.literals)
        else:
            if cls.literals:
                lines.append('  compartment "Valeurs" [kind=literal] {')
                lines.extend('    ' + _text(value, f'{cls.name} (valeur)') for value in cls.literals)
                lines.append('  }')
            lines.extend('  ' + _member_line(member, cls.name) for member in members)
        lines.append('}')

    for rel in diagram.relationships:
        source, target = by_id[rel.source], by_id[rel.target]
        context = f'{source.name} → {target.name}'
        arrow = {'association': '--', 'inheritance': '--|>', 'composition': '*--',
                 'aggregation': 'o--', 'provided_interface': '--'}[rel.kind]
        ends = []
        for mult, role, vis in ((rel.source_multiplicity, rel.source_role, rel.source_visibility),
                                (rel.target_multiplicity, rel.target_role, rel.target_visibility)):
            if rel.kind == 'inheritance' and mult is not None:
                warnings.add(f'Multiplicité d’héritage non représentable, omise ({context})')
                mult = None
            if vis is not None:
                warnings.add('Visibilités d’extrémité de relation non représentables en DSL Atelier, omises')
            mult = _multiplicity(mult, context, warnings)
            ends.append((f' {_string(mult)}' if mult is not None else '',
                         f' [role={_string(role)}]' if role is not None else ''))
        (lmult, lrole), (rmult, rrole) = ends
        label = ' : ' + _text(rel.name, f'Relation {context}') if rel.name is not None else ''
        lines.append(f'::{ids[rel.source]}{lmult}{lrole} {arrow}{rmult} ::{ids[rel.target]}{rrole}{label}')

    free = 0
    for note in diagram.notes:
        if note.targets:
            where = ' on ' + ', '.join(f'::{ids[t]}' for t in note.targets)
        else:
            # Le DSL exige une position pour une note libre ; le modèle n'en a pas.
            where = f' [at=(0pt,{free * _FREE_NOTE_STEP}pt)]'
            free += 1
            warnings.add('Notes sans cible placées à une position arbitraire (colonne à gauche)')
        lines.append(f'note{where} : {_text(note.text, f"Note {note.id}")}')

    diagram.warnings.extend(sorted(warnings))
    return '\n'.join(lines) + '\n'
