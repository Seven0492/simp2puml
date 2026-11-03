"""Sous-ensemble strict PlantUML, une instruction par ligne.

Classes, interfaces, enums, symboles ``()``, membres et relations sont pris en
charge ; les packages, réalisations et préprocesseur restent interdits. Une
multiplicité de membre est un suffixe séparé par un espace : ``T[] [0..*]``
ou ``nom [1]``. Les types finissant littéralement par un tel suffixe sont
ambigus et refusés à l'écriture. Les notes utilisent ``note as ID`` et des
liens ``ID .. classe``. Les flèches d'association perdent leur direction avec
avertissement ; les identifiants non ASCII simples sont remappés en alias.
"""
from contextlib import contextmanager
import re
from typing import NoReturn

from .model import (ConversionError, UMLAttribute, UMLClass, UMLDiagram, UMLNote,
                   UMLOperation, UMLParameter, UMLRelationship)

_ID = r'[A-Za-z_][A-Za-z0-9_]*'
_IDENT = re.compile(rf'{_ID}\Z')
_MEMBER = r'(?:[^\W\d]|_)\w*'
_MEMBER_IDENT = re.compile(rf'{_MEMBER}\Z')
_DECL = re.compile(rf'(class|abstract\s+class|interface|enum|\(\))\s+(?:"([^"\\\n]*)"\s+as\s+({_ID})|({_ID})\s+as\s+"([^"\\\n]*)"|({_ID}))\s*(\{{)?\s*\Z')
_END = r'(?:\s+"([^"\\\n]+)")?(?:/([+\-#~]?)([^\s/:]*))?'
_REL = re.compile(rf'({_ID}){_END}\s*(<\|--|--\|>|\*--|--\*|o--|--o|-->|<--|--|\.\.)\s*{_END}\s*({_ID})(?:\s*:\s*(.*))?\s*\Z')
_GRAPHIC = re.compile(r'(?:skinparam\s+[^{}]+|hide\s+[^{}]+|show\s+[^{}]+|scale\s+[^{}]+|page\s+[^{}]+|(?:left to right|top to bottom) direction)\Z', re.IGNORECASE)
_MULT = r'(?:\d+|\*|\d+\.\.(?:\d+|\*))'
_SUFFIX = re.compile(rf'\s+\[({_MULT})\]\Z')


def _error(message: str) -> NoReturn:
    raise ConversionError(message)


@contextmanager
def _at_line(number: int):
    try:
        yield
    except ConversionError as exc:
        raise ConversionError(f'Ligne {number}: {exc}') from exc


def _clean(text: str) -> list[str]:
    """Enlève les commentaires hors notes, sans altérer leur texte brut."""
    result = []
    block = False
    block_line = None
    note = False
    for number, raw in enumerate(text.split('\n'), 1):
        if note:
            result.append(raw)
            if raw.strip() == 'end note':
                note = False
            continue
        out = []
        quote = False
        i = 0
        while i < len(raw):
            if block:
                if raw.startswith("'/", i):
                    block = False
                    i += 2
                else:
                    i += 1
            elif not quote and raw.startswith("/'", i):
                block = True
                block_line = number
                out.append(' ')
                i += 2
            elif not quote and raw[i] == "'":
                break
            else:
                c = raw[i]
                if c == '"':
                    quote = not quote
                out.append(c)
                i += 1
        if quote:
            _error(f'Ligne {number}: guillemet non fermé')
        line = ''.join(out)
        result.append(line)
        if re.fullmatch(rf'\s*note\s+as\s+{_ID}\s*', line):
            note = True
    if block:
        _error(f'Ligne {block_line}: commentaire non fermé')
    return result


def _split(text: str, separator: str) -> list[str]:
    parts, stack = [], []
    quote = False
    start = 0
    pairs = {')': '(', ']': '[', '>': '<'}
    for i, c in enumerate(text):
        if c == '"':
            quote = not quote
        elif not quote:
            if c in '([<':
                stack.append(c)
            elif c in ')]>':
                if not stack or stack.pop() != pairs[c]:
                    _error('Délimiteurs déséquilibrés')
            elif c == separator and not stack:
                parts.append(text[start:i].strip())
                start = i + 1
    if quote or stack:
        _error('Délimiteurs déséquilibrés')
    parts.append(text[start:].strip())
    return parts


def _type(value: str) -> str:
    if not value or value != value.strip() or any(c in value for c in "\n\r{}'\\"):
        _error('Type non représentable')
    quoted = False
    for char in value:
        if char == '"':
            quoted = not quoted
        elif char == '=' and not quoted:
            _error('Valeur par défaut ou initialisateur non pris en charge')
    if len(_split(value, ',')) != 1:
        _error('Type ambigu')
    # Les seuls deux-points au niveau supérieur autorisés sont les paires ::.
    # Les deux-points isolés au niveau supérieur restent ambigus.
    depth = 0
    quoted = False
    for i, char in enumerate(value):
        if char == '"':
            quoted = not quoted
        elif not quoted:
            if char in '([<':
                depth += 1
            elif char in ')]>':
                depth -= 1
            elif char == ':' and depth == 0 and not ((i and value[i - 1] == ':') or (i + 1 < len(value) and value[i + 1] == ':')):
                _error('Type ambigu')
    return value


def _title(value: str) -> str:
    if (not value or value != value.strip() or any(c in value for c in "'\"\\") or
            any(ord(c) < 32 or ord(c) == 127 or c in '\u0085\u2028\u2029' for c in value)):
        _error('Titre non représentable')
    return value


def _multiplicity(text: str) -> tuple[str, str | None]:
    match = _SUFFIX.search(text)
    if match:
        # A suffix inside a quoted or nested type is not a multiplicity.
        prefix = text[:match.start()]
        try:
            _split(prefix, ',')
        except ConversionError:
            return text, None
        return prefix.rstrip(), match.group(1)
    return text, None


def _named(text: str) -> tuple[str, str | None, str | None]:
    text, mult = _multiplicity(text.strip())
    match = re.fullmatch(rf'({_MEMBER})(?:\s*:\s*(.+))?', text)
    if not match:
        _error('Nom ou type ambigu : ' + text)
    name, typ = match.groups()
    return name, _type(typ) if typ is not None else None, mult


def _member(text: str, cls: UMLClass) -> None:
    static = abstract = False
    visibility = None
    while True:
        text = text.strip()
        if text.startswith('{static}') and not static:
            static, text = True, text[8:]
        elif text.startswith('{abstract}') and not abstract:
            abstract, text = True, text[10:]
        elif text and text[0] in '+-#~' and visibility is None:
            visibility, text = text[0], text[1:]
        else:
            break
    text = text.strip()
    if not re.match(rf'{_MEMBER}\s*\(', text):
        if abstract:
            _error('Attribut abstrait non pris en charge')
        name, typ, mult = _named(text)
        cls.attributes.append(UMLAttribute(name, typ, visibility, static, mult))
        return
    match = re.match(rf'({_MEMBER})\s*\(', text)
    if not match:
        _error('Opération invalide : ' + text)
    name = match.group(1)
    start = match.end()
    depth, quoted, end = 1, False, None
    for i in range(start, len(text)):
        char = text[i]
        if char == '"':
            quoted = not quoted
        elif not quoted:
            if char == '(':
                depth += 1
            elif char == ')':
                depth -= 1
                if depth == 0:
                    end = i
                    break
    if end is None:
        _error('Opération invalide : ' + text)
    params, suffix = text[start:end], text[end + 1:]
    suffix, mult = _multiplicity(suffix)
    suffix = suffix.strip()
    if suffix and not suffix.startswith(':'):
        _error('Retour invalide')
    ret = _type(suffix[1:].strip()) if suffix else None
    parameters = []
    if params.strip():
        for part in _split(params, ','):
            pname, ptype, pmult = _named(part)
            parameters.append(UMLParameter(pname, ptype, pmult))
    cls.operations.append(UMLOperation(name, parameters, ret, visibility, static, abstract, mult))


def _note_text(text: str, first_line: int | None = None) -> str:
    if '\r' in text or any(c in text for c in '\u0085\u2028\u2029'):
        _error('Texte de note non représentable')
    for offset, line in enumerate(text.split('\n')):
        stripped = line.strip()
        if (stripped.lower() in ('end note', '@startuml', '@enduml') or
                stripped.startswith('!') or stripped.lower().startswith(('@startuml ', '@enduml '))):
            location = f'Ligne {first_line + offset}: ' if first_line is not None else ''
            _error(location + 'Directive interdite dans une note')
    return text


def _endpoint(mult: str | None, vis: str | None, role: str | None) -> tuple[str | None, str | None, str | None]:
    if mult is not None and (not mult or any(c in mult for c in '"\\\n\r') or mult != mult.strip()):
        _error('Multiplicité de relation non représentable')
    if role not in (None, '') and not _MEMBER_IDENT.fullmatch(role):
        _error('Rôle non représentable')
    if vis not in (None, '', '+', '-', '#', '~'):
        _error('Visibilité de rôle inconnue')
    return mult, vis or None, role or None


def parse_puml(text: str) -> UMLDiagram:
    lines = _clean(text.replace('\r\n', '\n').replace('\r', '\n'))
    diagram = UMLDiagram()
    classes, notes = {}, {}
    declared = set()
    pending_rel = []
    state, current, pending = 'before', None, None
    note_lines = []
    note_start = 0
    has_title = False
    for number, raw in enumerate(lines, 1):
        line = raw.strip()
        if state == 'note':
            assert isinstance(current, UMLNote)
            if line == 'end note':
                current.text = _note_text('\n'.join(note_lines), first_line=note_start + 1)
                state, current = 'diagram', None
            else:
                note_lines.append(raw)
            continue
        if not line:
            continue
        if state == 'before':
            if line != '@startuml':
                _error(f'Ligne {number}: @startuml attendu')
            state = 'diagram'
            continue
        if state == 'after':
            _error(f'Ligne {number}: contenu après @enduml')
        if state == 'body':
            assert isinstance(current, UMLClass)
            if line == '}':
                state, current = 'diagram', None
            elif current.kind == 'enumeration' and _MEMBER_IDENT.fullmatch(line):
                current.literals.append(line)
            else:
                with _at_line(number):
                    _member(line, current)
            continue
        if line == '{' and pending is not None:
            current, pending, state = pending, None, 'body'
            continue
        pending = None
        if line == '@enduml':
            state = 'after'
            continue
        if line == 'title' or line.startswith(('title ', 'title\t')):
            if has_title:
                _error(f'Ligne {number}: titre dupliqué')
            with _at_line(number):
                diagram.name = _title(line[5:].strip())
            has_title = True
            continue
        match = re.fullmatch(rf'note\s+as\s+({_ID})', line)
        if match:
            ident = match.group(1)
            if ident in declared or ident in notes or ident in classes:
                _error(f'Ligne {number}: identifiant de note dupliqué')
            current = UMLNote(ident, '')
            notes[ident] = current
            diagram.notes.append(current)
            note_lines = []
            note_start = number
            state = 'note'
            continue
        match = _DECL.fullmatch(line)
        if match:
            keyword, quoted1, alias1, alias2, quoted2, plain, opening = match.groups()
            ident = alias1 or alias2 or plain
            name = quoted1 if alias1 else quoted2 if alias2 else plain
            if not name or name != name.strip() or ident in declared or ident in notes:
                _error(f'Ligne {number}: classe invalide ou dupliquée')
            kind = {'class': 'class', 'abstract class': 'class', 'interface': 'interface', 'enum': 'enumeration', '()': 'interface_symbol'}[re.sub(r'\s+', ' ', keyword)]
            if kind == 'interface_symbol' and opening:
                _error(f'Ligne {number}: un symbole ne peut avoir de corps')
            declared.add(ident)
            if ident in classes:
                cls = classes[ident]
                cls.name, cls.kind, cls.is_abstract = name, kind, keyword.startswith('abstract')
            else:
                cls = UMLClass(id=ident, name=name, kind=kind, is_abstract=keyword.startswith('abstract'))
                classes[ident] = cls
                diagram.classes.append(cls)
            if opening:
                current, state = cls, 'body'
            elif kind != 'interface_symbol':
                pending = cls
            continue
        match = _REL.fullmatch(line)
        if match:
            left, lm, lv, lr, arrow, rm, rv, rr, right, label = match.groups()
            with _at_line(number):
                if label is not None:
                    _relation_name(label)
                _endpoint(lm, lv, lr)
                _endpoint(rm, rv, rr)
                if (lv == '' and lr == '') or (rv == '' and rr == ''):
                    _error('Rôle vide')
            if arrow != '..':
                for ident in (left, right):
                    if ident in notes:
                        _error(f'Ligne {number}: relation de note invalide')
                    if ident not in classes:
                        cls = UMLClass(id=ident, name=ident)
                        classes[ident] = cls
                        diagram.classes.append(cls)
            pending_rel.append((number, left, right, arrow, lm, lv or None, lr or None, rm, rv or None, rr or None, label))
            continue
        if _GRAPHIC.fullmatch(line):
            diagram.warnings.append(f'Ligne {number}: directive graphique ignorée : {line}')
            continue
        _error(f'Ligne {number}: syntaxe non prise en charge : {line}')
    if state != 'after':
        _error('Diagramme incomplet : @enduml, note ou accolade manquante')
    for number, left, right, arrow, lm, lv, lr, rm, rv, rr, label in pending_rel:
        if arrow == '..':
            if label is not None or any(x is not None for x in (lm, lv, lr, rm, rv, rr)):
                _error(f'Ligne {number}: lien de note annoté non pris en charge')
            if left in notes and right not in notes:
                note, target = notes[left], right
            elif right in notes and left not in notes:
                note, target = notes[right], left
            else:
                _error(f'Ligne {number}: lien de note invalide')
            if target not in classes:
                _error(f'Ligne {number}: cible de note inconnue')
            note.targets.append(target)
            continue
        for ident in (left, right):
            if ident in notes:
                _error(f'Ligne {number}: relation de note invalide')
            if ident not in classes:
                cls = UMLClass(id=ident, name=ident)
                classes[ident] = cls
                diagram.classes.append(cls)
        kind = {'<|--': 'inheritance', '--|>': 'inheritance', '*--': 'composition', '--*': 'composition', 'o--': 'aggregation', '--o': 'aggregation'}.get(arrow, 'association')
        reverse = arrow in ('<|--', '--*', '--o')
        if kind == 'association':
            if arrow != '--':
                diagram.warnings.append(f'Ligne {number}: direction de {arrow} perdue')
            ls = classes[left].kind == 'interface_symbol'
            rs = classes[right].kind == 'interface_symbol'
            if ls and rs:
                _error(f'Ligne {number}: relation entre deux symboles non représentable')
            if ls or rs:
                kind, reverse = 'provided_interface', ls
        if reverse:
            left, right, lm, rm, lv, rv, lr, rr = right, left, rm, lm, rv, lv, rr, lr
        diagram.relationships.append(UMLRelationship(left, right, kind, label, lm, rm, lr, rr, lv, rv))
    diagram.validate()
    return diagram


def _relation_name(value: str) -> str:
    if (not value or value != value.strip() or any(c in value for c in "'\"\\\n\r") or
            any(ord(c) < 32 or ord(c) == 127 for c in value)):
        _error('Nom de relation non représentable')
    return value


def _member_suffix(typ: str | None, mult: str | None) -> str:
    if typ is not None:
        _type(typ)
        if _multiplicity(typ)[1] is not None:
            _error('Type terminé par une multiplicité ambiguë')
    if mult is not None and not re.fullmatch(_MULT, mult):
        _error('Multiplicité de membre non représentable')
    return (': ' + typ if typ is not None else '') + (' [' + mult + ']' if mult is not None else '')


def write_puml(diagram: UMLDiagram) -> str:
    if diagram.source_view is not None:
        from .simp_puml import write_view
        content, warnings = write_view(diagram.source_view)
        diagram.warnings.extend(w for w in warnings if w not in diagram.warnings)
        return content
    diagram.validate()
    _title(diagram.name)
    lines = ['@startuml']
    if diagram.name != 'Diagram':
        lines.append(f'title {diagram.name}')
    objects = [*diagram.classes, *diagram.notes]
    reserved = {obj.id for obj in objects if _IDENT.fullmatch(obj.id)}
    aliases = {}
    next_alias = 1
    for obj in objects:
        if _IDENT.fullmatch(obj.id):
            alias = obj.id
        else:
            while f'C{next_alias}' in reserved:
                next_alias += 1
            alias = f'C{next_alias}'
            next_alias += 1
            reserved.add(alias)
        aliases[obj.id] = alias
    for cls in diagram.classes:
        if not cls.name or cls.name != cls.name.strip() or any(c in cls.name for c in '"\\\n\r'):
            _error('Nom de classe non représentable')
        if cls.is_abstract and cls.kind != 'class':
            _error('Classifieur abstrait non représentable')
        keyword = {'class': 'abstract class' if cls.is_abstract else 'class', 'interface': 'interface', 'enumeration': 'enum', 'interface_symbol': '()'}[cls.kind]
        declaration = f'{keyword} "{cls.name}" as {aliases[cls.id]}'
        if cls.kind == 'interface_symbol':
            lines.append(declaration)
            continue
        lines.append(declaration + ' {')
        for literal in cls.literals:
            if not _MEMBER_IDENT.fullmatch(literal):
                _error('Valeur d’énumération non représentable')
            lines.append('  ' + literal)
        for member in [*cls.attributes, *cls.operations]:
            if not _MEMBER_IDENT.fullmatch(member.name) or member.visibility not in (None, '+', '-', '#', '~'):
                _error('Nom ou visibilité de membre non représentable')
            prefix = (member.visibility or '') + ('{static} ' if member.is_static else '')
            if isinstance(member, UMLAttribute):
                if cls.kind == 'enumeration' and member.type is None and member.visibility is None and not member.is_static and member.multiplicity is None:
                    _error('Attribut d’énumération ambigu avec une valeur')
                lines.append(f'  {prefix}{member.name}{_member_suffix(member.type, member.multiplicity)}')
            else:
                if member.is_abstract:
                    prefix += '{abstract} '
                params = []
                for param in member.parameters:
                    if not _MEMBER_IDENT.fullmatch(param.name):
                        _error('Nom de paramètre non représentable')
                    params.append(param.name + _member_suffix(param.type, param.multiplicity))
                lines.append(f'  {prefix}{member.name}({", ".join(params)}){_member_suffix(member.return_type, member.multiplicity)}')
        lines.append('}')
    by_id = {cls.id: cls for cls in diagram.classes}
    for rel in diagram.relationships:
        source_symbol = by_id[rel.source].kind == 'interface_symbol'
        target_symbol = by_id[rel.target].kind == 'interface_symbol'
        if rel.kind == 'association' and (source_symbol or target_symbol):
            _error('Association avec symbole ambiguë avec un connecteur fourni')
        if rel.kind == 'provided_interface' and (source_symbol or not target_symbol):
            _error('Connecteur fourni invalide')
        arrow = {'association': '--', 'inheritance': '--|>', 'composition': '*--', 'aggregation': 'o--', 'provided_interface': '--'}[rel.kind]
        def end(mult, role, vis):
            _endpoint(mult, vis, role)
            return (' "' + mult + '"' if mult is not None else '') + ('/' + (vis or '') + (role or '') if role is not None or vis is not None else '')
        left = end(rel.source_multiplicity, rel.source_role, rel.source_visibility)
        right = end(rel.target_multiplicity, rel.target_role, rel.target_visibility)
        label = ' : ' + _relation_name(rel.name) if rel.name is not None else ''
        lines.append(f'{aliases[rel.source]}{left} {arrow}{right} {aliases[rel.target]}{label}')
    for note in diagram.notes:
        _note_text(note.text)
        lines.append(f'note as {aliases[note.id]}')
        lines.extend(note.text.split('\n'))
        lines.append('end note')
        for target in note.targets:
            lines.append(f'{aliases[note.id]} .. {aliases[target]}')
    lines.append('@enduml')
    return '\n'.join(lines) + '\n'
