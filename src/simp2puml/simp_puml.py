"""Adaptateurs PlantUML des vues SIMP enrichies.

Les notations structurelles et les séquences ont des générateurs distincts.
Les familles sans équivalent sont projetées en graphe annoté, avec diagnostic.
Le XML des éléments sélectionnés est archivé en commentaires JSON inertes :
ce n'est ni une instruction PlantUML, ni une promesse d'import inverse.
"""
import json
import math
import re
import textwrap
import xml.etree.ElementTree as ET

from .model import ConversionError
from .simp_document import SIMPView

_VIS = {'public': '+', 'private': '-', 'protected': '#', 'package': '~'}
_NODES = {
    'class': 'class', 'uml-interface-classifier': 'interface', 'enumeration': 'enum',
    'uml-interface': '()', 'data-type': 'class', 'signal': 'class',
    'actor': 'actor', 'use-case': 'usecase', 'package': 'package', 'group': 'rectangle',
    'system-boundary': 'rectangle', 'boundary': 'rectangle', 'node': 'node',
    'device': 'node', 'execution-environment': 'node', 'component': 'component',
    'artifact': 'artifact', 'object': 'object', 'erd-entity': 'entity',
    'uml-n-ary-association': 'diamond', 'collaboration': 'usecase',
    'collaboration-use': 'usecase', 'port': 'rectangle', 'part': 'rectangle',
    'comment': 'note', 'formatted-text': 'note', 'text': 'note',
}
_ARROWS = {
    'generalization': '--|>', 'realization': '..|>', 'interface-realization': '..|>',
    'dependency': '..>', 'usage': '..>', 'abstraction': '..>', 'substitution': '..|>',
    'manifestation': '..>', 'deployment': '..>', 'include': '..>', 'extend': '..>',
    'relationship': '--', 'connector': '--', 'communication-path': '--',
    'uml-provided-interface-connector': '--', 'uml-required-interface-connector': '..>',
    'comment-connector': '..', 'containment': '+--', 'template-binding': '..>',
    'general-transition': '-->', 'control-flow': '-->', 'object-flow': '-->',
    'flow': '-->', 'transition': '-->', 'erd-relationship': '--',
    'association-class-link': '..',
}
_STEREO_REL = {'include', 'extend', 'usage', 'abstraction', 'substitution',
               'manifestation', 'deployment', 'template-binding'}
_STATE_TYPES = {'state', 'initial-state', 'final-state', 'decision', 'merge', 'fork',
                'join', 'history', 'deep-history', 'entry-point', 'exit-point',
                'terminate', 'choice', 'junction', 'activity-final', 'flow-final'}
_STATE_STEREO = {'initial-state': 'start', 'final-state': 'end', 'activity-final': 'end',
                 'flow-final': 'end', 'decision': 'choice', 'merge': 'choice',
                 'choice': 'choice', 'junction': 'choice', 'fork': 'fork', 'join': 'join',
                 'history': 'history', 'deep-history': 'history*', 'entry-point': 'entryPoint',
                 'exit-point': 'exitPoint', 'terminate': 'end'}
_STRUCTURAL = {'uml-class', 'uml-usecase', 'uml-component', 'uml-deployment', 'uml-object',
               'uml-package', 'uml-composite', 'uml-composite-structure', 'entity-relationship'}
_NOTE_TYPES = {'comment', 'formatted-text', 'text'}
_CLASSIFIERS = {'class', 'abstract class', 'interface', 'enum'}
# Nom générique « Nom<T, U> » : paramètres limités à des caractères sans effet en PlantUML.
_GENERIC = re.compile(r'^([^<>]+?)<([\w ,.?]+)>$')
# Largeur moyenne d'un caractère (px) pour couper les notes à la largeur de leur boîte SIMP.
_NOTE_CHAR_PX = 6.2


def wrap_note(text: str, width: float | None) -> str:
    """Coupe une note à la largeur de sa boîte dans SIMP (les retours à la ligne existants sont gardés)."""
    text = text.strip()
    if not width or width <= 0:
        return text
    columns = max(16, round(width / _NOTE_CHAR_PX))
    lines = []
    for line in text.splitlines():
        lines.extend(textwrap.wrap(line, columns, break_long_words=False, break_on_hyphens=False) or [''])
    return '\n'.join(lines)


def literal(value: str) -> str:
    """Échappe du texte utilisateur, y compris les fonctions du préprocesseur.

    Les sauts de ligne restent du texte (\\n) et ne peuvent ouvrir une directive.
    Les liens Creole, balises d'images et expressions %... ne sont pas exécutés.
    """
    result = []
    for c in value.replace('\r\n', '\n').replace('\r', '\n'):
        if c == '\n':
            result.append(r'\n')
        elif c in '"\\<>[]{}%@!~' or ord(c) < 32 or ord(c) == 127:
            result.append(f'<U+{ord(c):04X}>')
        else:
            result.append(c)
    return ''.join(result)


def _archive_json(data) -> str:
    text = json.dumps(data, ensure_ascii=True)
    for c in '@!%':
        text = text.replace(c, f'\\u{ord(c):04x}')
    return text


def _true(node: ET.Element, *keys: str) -> bool:
    return any(node.get(k, '').lower() in ('true', '1') for k in keys)


class _Writer:
    def __init__(self, view: SIMPView):
        self.view = view
        self.ids = {ident: f'E{i}' for i, ident in enumerate(view.elements, 1)}
        self.lines = ['@startuml', f'title {literal(view.name)}']
        self.warnings: list[str] = []

    def warn(self, message: str) -> None:
        if message not in self.warnings:
            self.warnings.append(message)

    def graphic(self, ident: str) -> ET.Element | None:
        graphics = self.view.graphics.get(ident, [])
        if len(graphics) > 1:
            self.warn(f'Occurrences multiples réunies pour {ident} ; positions distinctes non reproduites')
        return graphics[0] if graphics else None

    def number(self, ident: str, key: str, default: float = 0) -> float:
        graphic = self.graphic(ident)
        layout = graphic.find('layout') if graphic is not None else None
        raw = layout.get(key) if layout is not None else None
        if raw is None:
            return default
        try:
            number = float(raw)
        except ValueError as exc:
            raise ConversionError(f'Coordonnée {key} invalide pour {ident} : {raw!r}') from exc
        if not math.isfinite(number):
            raise ConversionError(f'Coordonnée {key} non finie pour {ident}')
        return number

    def box(self, ident: str) -> tuple[float, float, float, float] | None:
        """Position et taille (x, y, largeur, hauteur) dans le dessin SIMP, si elles sont connues."""
        graphic = self.graphic(ident)
        layout = graphic.find('layout') if graphic is not None else None
        if layout is None or any(layout.get(k) is None for k in ('x', 'y', 'width', 'height')):
            return None
        return tuple(self.number(ident, k) for k in ('x', 'y', 'width', 'height'))

    def note_side(self, note: str, target: str) -> str:
        """Côté de la cible où se trouvait la note dans SIMP (left, right, top ou bottom)."""
        a, b = self.box(note), self.box(target)
        if a is None or b is None:
            return 'right'
        dx = (a[0] + a[2] / 2) - (b[0] + b[2] / 2)
        dy = (a[1] + a[3] / 2) - (b[1] + b[3] / 2)
        # Distances rapportées aux demi-tailles cumulées : une note large reste « au-dessus » si elle l'était.
        nx, ny = dx / max(1.0, (a[2] + b[2]) / 2), dy / max(1.0, (a[3] + b[3]) / 2)
        if abs(nx) >= abs(ny):
            return 'right' if dx > 0 else 'left'
        return 'bottom' if dy > 0 else 'top'

    def declaration(self, keyword: str, name: str, alias: str, stereotype: str) -> str:
        """Déclaration PlantUML ; un classifieur générique utilise la notation UML <T> (cadre pointillé)."""
        match = _GENERIC.match(name) if keyword in _CLASSIFIERS else None
        if match:
            params = ', '.join(p.strip() for p in match.group(2).split(','))
            return f'{keyword} "{literal(match.group(1).strip())}" as {alias} <{params}>{stereotype}'
        return f'{keyword} "{literal(name)}" as {alias}{stereotype}'

    def endpoints(self, ident: str, item: ET.Element) -> tuple[str | None, str | None]:
        nodes = [item]
        for tag in ('relation', 'sequence-action'):
            node = item.find(tag)
            if node is not None:
                nodes.insert(0, node)
        graphic = self.graphic(ident)
        if graphic is not None:
            for tag in ('relation', 'sequence-action'):
                node = graphic.find(tag)
                if node is not None:
                    nodes.append(node)
        source = next((n.get('from') for n in nodes if n.get('from')), None)
        target = next((n.get('to') for n in nodes if n.get('to')), None)
        return source, target

    def stereotypes(self, item: ET.Element) -> str:
        values = [n.get('name', '') for n in item.findall('./stereotypes/stereotype')]
        typ = item.get('type', '')
        if typ in ('data-type', 'signal', 'collaboration', 'collaboration-use'):
            values.insert(0, typ)
        return ''.join(f' <<{literal(value)}>>' for value in dict.fromkeys(values) if value)

    def type_name(self, value: str, ref: str) -> str:
        if ref.startswith('Class:'):
            ident = ref[6:]
            if ident not in self.view.available_ids:
                raise ConversionError(f'Type référencé introuvable : {ref}')
            target = self.view.elements.get(ident)
            if target is None:
                self.warn(f'Type externe conservé comme référence textuelle : {ref}')
            elif sum(e.get('name') == target.get('name') for e in self.view.elements.values()
                     if e.get('type') in ('class', 'uml-interface-classifier', 'enumeration', 'data-type')) > 1:
                self.warn(f'Type {ref} : nom homonyme dans le dessin ; identité conservée en commentaire XML')
            return value or (target.get('name') if target is not None else None) or ref
        return value or ref.removeprefix('Uml.')

    def member(self, node: ET.Element, operation: bool = False) -> str:
        prop = self.view.elements.get(node.get('property-ref', ''))
        feature = prop.find('structural-feature') if prop is not None else None
        nodes = [node] + ([prop] if prop is not None else []) + ([feature] if feature is not None else [])
        if prop is not None:
            nodes += prop.findall('feature') + prop.findall('property')
        def value(*keys):
            return next((n.get(k) for n in nodes for k in keys
                         if not (n is prop and k in ('type', 'type-id')) and n.get(k) not in (None, '')), '')
        def flag(*keys):
            return any(_true(n, *keys) for n in nodes)
        name = literal(node.get('name') or (prop.get('name', '') if prop is not None else ''))
        visibility = value('visibility')
        if visibility and visibility not in _VIS:
            raise ConversionError(f'Visibilité SIMP inconnue : {visibility}')
        prefix = _VIS.get(visibility, '')
        if flag('static', 'is-static'):
            prefix += '{static} '
        if flag('abstract', 'is-abstract'):
            prefix += '{abstract} '
        typ = self.type_name(value('type'), value('type-id'))
        if operation:
            params = []
            for p in node.findall('./parameters/parameter'):
                text = p.get('name', '')
                if p.get('direction'):
                    text = p.get('direction', '') + ' ' + text
                ptype = self.type_name(p.get('type', ''), p.get('type-id', ''))
                if ptype:
                    text += ': ' + ptype
                if p.get('multiplicity'):
                    text += ' [' + p.get('multiplicity', '') + ']'
                if p.get('default-value'):
                    text += ' = ' + p.get('default-value', '')
                params.append(literal(text))
            name += '(' + ', '.join(params) + ')'
        result = prefix + name + (': ' + literal(typ) if typ else '')
        mult = value('multiplicity')
        if mult:
            result += ' [' + literal(mult) + ']'
        initial = value('default-value', 'initial-value', 'value')
        if initial:
            result += ' = ' + literal(initial)
        flags = [label for keys, label in ((('read-only', 'is-read-only'), 'readOnly'),
                 (('derived', 'is-derived'), 'derived'), (('ordered', 'is-ordered'), 'ordered'),
                 (('unique', 'is-unique'), 'unique'), (('is-id', 'primary-key'), 'id'),
                 (('foreign-key',), 'FK')) if flag(*keys)]
        if flags:
            result += ' ' + literal('{' + ', '.join(flags) + '}')
        return result

    def details(self, item: ET.Element) -> list[str]:
        lines = []
        for literal_ in item.findall('./enumeration-items/enumeration-item'):
            lines.append(literal(literal_.get('name', '')))
        for attr in item.findall('./attributes/attribute'):
            lines.append('{field} ' + self.member(attr))
        for op in item.findall('./operations/operation'):
            lines.append('{method} ' + self.member(op, True))
        for attr in item.findall('./object/attributes/attribute'):
            lines.append(literal(attr.get('name', '') + ' = ' + attr.get('value', '')))
        for action in item.findall('./actions/*'):
            lines.append(literal(action.get('type', action.tag) + ' / ' + (action.get('name') or action.text or '')))
        return lines

    def edge_label(self, item: ET.Element) -> str:
        label = item.get('name', '')
        transition = item.find('transition')
        if transition is not None:
            guard = transition.get('guard', '')
            behaviour = transition.get('behaviourExpression', '') or transition.get('effect', '')
            label += (' [' + guard + ']' if guard else '') + (' / ' + behaviour if behaviour else '')
        stereotypes = self.stereotypes(item)
        typ = item.get('type')
        if typ in _STEREO_REL and not stereotypes:
            stereotypes = f' <<{typ}>>'
        return stereotypes.strip() + (' ' if stereotypes and label else '') + literal(label)

    def structural(self) -> None:
        state_mode = self.view.kind in ('uml-statechart', 'uml-state-machine', 'uml-activity')
        if self.view.kind == 'uml-activity':
            self.warn('Activité : graphe des flux conservé en notation d’états ; partitions et symboles d’action approximés')
        elif self.view.kind not in _STRUCTURAL and not state_mode:
            self.warn(f'Type de diagramme {self.view.kind} : représentation générique annotée, notation spécifique non reproduite')
        nodes = {}
        edges = {}
        for ident, item in self.view.elements.items():
            if ident in self.view.omitted_relations:
                continue
            source, target = self.endpoints(ident, item)
            typ = item.get('type', '')
            if typ == 'property':
                continue
            if typ in _ARROWS or source or target:
                if not source or not target:
                    raise ConversionError(f'Extrémité de relation introuvable : {ident} ({source} → {target})')
                edges[ident] = (item, source, target)
            else:
                nodes[ident] = item
        parents = {}
        for ident, item in nodes.items():
            graphic = self.graphic(ident)
            parent = self.view.containers.get(ident) or item.get('owner')
            if parent in nodes and parent != ident:
                parents[ident] = parent
            elif parent == ident:
                raise ConversionError(f'Conteneur cyclique : {ident}')
            elif parent:
                if parent not in self.view.available_ids:
                    raise ConversionError(f'Conteneur introuvable : {parent} ({ident})')
                self.warn(f'Conteneur {parent} hors vue : élément {ident} placé à la racine')
        for ident in nodes:
            seen = {ident}
            parent = parents.get(ident)
            while parent is not None:
                if parent in seen:
                    raise ConversionError(f'Conteneur cyclique : {ident}')
                seen.add(parent)
                parent = parents.get(parent)
        children: dict[str | None, list[str]] = {}
        for ident in nodes:
            children.setdefault(parents.get(ident), []).append(ident)

        # Notes reliées à un seul élément : accrochées à côté de lui (note left of …), comme dans SIMP,
        # au lieu d'un nœud isolé relié par un long pointillé qui élargit tout le diagramme.
        note_links: dict[str, list[tuple[str, str]]] = {}
        for ident, (item, source, target) in edges.items():
            if item.get('type') == 'comment-connector':
                for note, other in ((source, target), (target, source)):
                    if nodes.get(note) is not None and nodes[note].get('type') in _NOTE_TYPES:
                        note_links.setdefault(note, []).append((ident, other))
        attached: dict[str, str] = {}
        for note, links in note_links.items():
            others = {other for _, other in links}
            if len(others) == 1:
                other = next(iter(others))
                if (other in nodes and nodes[other].get('type') not in _NOTE_TYPES and not state_mode
                        and note not in parents and other not in parents and not children.get(note)):
                    attached[note] = other
        attached_edges = {edge for note in attached for edge, _ in note_links[note]}

        def note_text(ident: str, item: ET.Element) -> str:
            raw = item.findtext('comment') or '\n'.join(item.itertext()).strip() or item.get('name', '')
            box = self.box(ident)
            return wrap_note(raw, box[2] if box else None)

        def emit(ident: str, depth: int = 0) -> None:
            if depth > 100:
                raise ConversionError('Imbrication de conteneurs supérieure à 100')
            if ident in attached:
                return  # Écrite après sa cible.
            item = nodes[ident]
            typ = item.get('type', '')
            name = item.get('name', '') or typ
            alias = self.ids[ident]
            details = self.details(item)
            stereotype = self.stereotypes(item)
            if state_mode:
                keyword = 'state'
                if typ in _STATE_STEREO:
                    stereotype += ' <<' + _STATE_STEREO[typ] + '>>'
                elif typ not in _STATE_TYPES and typ not in ('comment', 'formatted-text'):
                    stereotype += ' <<' + literal(typ) + '>>'
                    if self.view.kind != 'uml-activity':
                        self.warn(f'Élément {ident} ({typ}) : représentation générique en état annoté')
            else:
                keyword = _NODES.get(typ, 'rectangle')
                if typ not in _NODES:
                    stereotype += ' <<' + literal(typ) + '>>'
                    self.warn(f'Élément {ident} ({typ}) : représentation générique annotée')
            if typ in _NOTE_TYPES:
                keyword = 'note'
                name = note_text(ident, item)
                stereotype = ''
            if typ == 'object':
                obj = item.find('object')
                if obj is not None and obj.get('classifier-name'):
                    name += ' : ' + obj.get('classifier-name', '')
            if keyword == 'class' and _true(item, 'is-abstract', 'abstract'):
                keyword = 'abstract class'
            declaration = self.declaration(keyword, name, alias, stereotype)
            if children.get(ident):
                # PlantUML n'autorise pas un conteneur sur chaque symbole.
                if keyword not in ('package', 'rectangle', 'node', 'component', 'artifact', 'state'):
                    self.warn(f'Conteneur {ident} ({typ}) : forme remplacée par un rectangle')
                    declaration = f'rectangle "{literal(name)}" as {alias}{stereotype}'
                self.lines.append(declaration + ' {')
                for child in children[ident]:
                    emit(child, depth + 1)
                self.lines.append('}')
                if details:
                    self.lines.append(f'note right of {alias} : ' + r'\n'.join(details))
            elif details and keyword in ('class', 'abstract class', 'interface', 'enum', 'entity', 'object'):
                self.lines.append(declaration + ' {')
                self.lines.extend('  ' + line for line in details)
                self.lines.append('}')
            else:
                self.lines.append(declaration)
                for detail in details:
                    if keyword == 'state':
                        self.lines.append(f'{alias} : {detail}')
                    else:
                        self.lines.append(f'note right of {alias} : {detail}')
            extra = self.extra_properties(item)
            if extra:
                self.lines.append(f'note right of {alias} : ' + literal('\n'.join(extra)))

        for ident in children.get(None, []):
            emit(ident)
        for note, target in attached.items():
            side = self.note_side(note, target)
            self.lines.append(f'note {side} of {self.ids[target]} : {literal(note_text(note, nodes[note]))}')
        for ident, (item, source, target) in sorted(edges.items(), key=lambda pair: pair[1][0].get('type') == 'association-class-link'):
            if ident in attached_edges:
                continue  # Remplacé par la note accrochée à sa cible.
            if item.get('type') == 'association-class-link':
                edge_id, class_id = (source, target) if source in edges else (target, source)
                if edge_id in edges and class_id in nodes:
                    _, edge_from, edge_to = edges[edge_id]
                    if edge_from in nodes and edge_to in nodes:
                        self.lines.append(f'({self.ids[edge_from]}, {self.ids[edge_to]}) .. {self.ids[class_id]}')
                        continue
            if source not in nodes or target not in nodes:
                if source not in self.view.available_ids or target not in self.view.available_ids:
                    raise ConversionError(f'Extrémité de relation introuvable : {ident} ({source} → {target})')
                self.warn(f'Relation {ident} vers élément hors vue omise du dessin : {source} → {target} ; XML conservé')
                continue
            typ = item.get('type', '')
            arrow = _ARROWS.get(typ, '-->')
            if typ not in _ARROWS:
                self.warn(f'Relation {ident} ({typ}) : connecteur générique orienté')
            relation = item.find('relation')
            left = right = ''
            if relation is not None:
                roles = [relation.find('start-role'), relation.find('end-role')]
                diamonds = [r.get('association-type') if r is not None else None for r in roles]
                ends = []
                for role in roles:
                    text = ''
                    if role is not None:
                        if role.get('visibility') not in (None, '', *_VIS):
                            raise ConversionError(f'Visibilité SIMP inconnue : {role.get("visibility")}')
                        # La visibilité qualifie un nom de rôle : seule (valeur par défaut de SIMP),
                        # elle ne produirait qu'un « - » isolé à l'extrémité du lien.
                        name = role.get('name', '')
                        text = _VIS.get(role.get('visibility', ''), '') + name if name else ''
                        text += ('\n' if text and role.get('multiplicity') else '') + role.get('multiplicity', '')
                    ends.append(f' "{literal(text)}"' if text else '')
                left, right = ends
                tip = {'aggregation': 'o', 'composition': '*'}
                start = tip.get(diamonds[0], '')
                end = tip.get(diamonds[1], '')
                nav = [r.get('navigability', '').lower() if r is not None else '' for r in roles]
                if not start and nav[0] in ('true', 'navigable', '1'):
                    start = '<'
                if not end and nav[1] in ('true', 'navigable', '1'):
                    end = '>'
                arrow = start + '--' + end
                for n in nav:
                    if n not in ('', 'true', 'false', 'navigable', 'non-navigable', '1', '0'):
                        self.warn(f'Relation {ident} : navigabilité inconnue {n!r} conservée en commentaire XML')
                if any(n in ('false', 'non-navigable', '0') for n in nav):
                    self.warn(f'Relation {ident} : non-navigabilité explicite conservée en commentaire XML')
                for role in roles:
                    if role is not None and role.get('attribute-ref'):
                        self.warn(f'Relation {ident} : attribute-ref {role.get("attribute-ref")} conservé en commentaire XML')
            erd = item.find('erd-relation')
            if erd is not None:
                left = ' "' + literal(erd.get('multiplicity-from', '')) + '"'
                right = ' "' + literal(erd.get('multiplicity-to', '')) + '"'
            label = self.edge_label(item)
            if relation is not None and _true(relation, 'derived'):
                label = '/' + label
            self.lines.append(f'{self.ids[source]}{left} {arrow}{right} {self.ids[target]}' + (f' : {label}' if label else ''))
            extra = self.extra_properties(item)
            if extra:
                self.lines.append('note on link : ' + literal('\n'.join(extra)))

    def extra_properties(self, item: ET.Element) -> list[str]:
        """Expose les propriétés sémantiques sans traduction dédiée, avec alerte.

        Les styles, métadonnées d'édition et le XML complet restent archivés.
        """
        result = []
        ignored = {'style', 'stereotypes', 'stereotype-instances', 'owned-items', 'associated-diagrams',
                   'cooperation', 'source-codes', 'attributes', 'operations', 'enumeration-items',
                   'object', 'relation', 'transition', 'comment', 'actions', 'state-invariant',
                   'structural-feature', 'feature', 'property', 'erd-relation'}
        for child in item:
            if child.tag in ignored:
                continue
            def semantic_nodes(node):
                if node.tag in ('style', 'styles', 'page', 'margins', 'solid-brush'):
                    return
                yield node
                for descendant in node:
                    yield from semantic_nodes(descendant)
            text = []
            for node in semantic_nodes(child):
                for key, value in node.attrib.items():
                    if value and key not in ('id', 'uid', 'ref', 's', 'index', 'width', 'height', 'line-spacing', 'size'):
                        result.append(f'{node.tag}.{key} = {value}')
                if node.text and node.text.strip():
                    text.append(node.text.strip())
            if text:
                result.append(' '.join(text))
        for key in ('is-active', 'is-leaf', 'is-root'):
            if _true(item, key):
                result.append(key + ' = true')
        item_keys = {'id', 'uid', 'cid', 'type', 'name', 'owner', 'from', 'to', 'visibility',
                     'is-abstract', 'abstract', 'is-active', 'is-leaf', 'is-root', 'style-class',
                     'creator', 'creation-date', 'modifier', 'modification-date', 'change-tracking-state'}
        for key, value in item.attrib.items():
            if key not in item_keys and value:
                result.append(f'{key} = {value}')
        # Les membres ont leur propre noyau traduit. Toute extension conserve
        # sa valeur lisible au lieu de disparaître derrière le XML d'archive.
        # Le profil 15.1 date aussi chaque membre : métadonnées d'édition, comme au niveau des éléments.
        member_keys = {'id', 'uid', 'cid', 'name', 'type', 'type-id', 'visibility',
                       'creator', 'creation-date', 'modifier', 'modification-date', 'change-tracking-state',
                       'property-ref', 'static', 'is-static', 'abstract', 'is-abstract',
                       'multiplicity', 'default-value', 'initial-value', 'value',
                       'read-only', 'is-read-only', 'derived', 'is-derived',
                       'ordered', 'is-ordered', 'unique', 'is-unique', 'is-id',
                       'primary-key', 'foreign-key', 'direction'}
        for path in ('./attributes/attribute', './operations/operation', './operations/operation/parameters/parameter'):
            for member in item.findall(path):
                for key, value in member.attrib.items():
                    if key not in member_keys and value not in ('', 'false', '0'):
                        result.append(f'{member.get("name", member.tag)}.{key} = {value}')
        if result:
            self.warn(f'Élément {item.get("id")} : propriétés supplémentaires représentées en annotation')
        return result

    def sequence(self) -> None:
        view = self.view
        messages = {i: e for i, e in view.elements.items() if e.get('type') == 'sequence-action'}
        activations = {i: e for i, e in view.elements.items() if e.get('type') == 'activation-bar'}
        participants = {}
        for ident, item in view.elements.items():
            graphic = self.graphic(ident)
            if item.get('type') in ('actor', 'lifeline') or (graphic is not None and graphic.get('type') == 'lifeline'):
                participants[ident] = item
        if not participants and not messages:
            self.warn('Séquence sans ligne de vie : représentation structurelle annotée')
            self.structural()
            return
        def owner(ident):
            seen = set()
            while ident in activations:
                if ident in seen:
                    raise ConversionError(f'Propriétaire d’activation cyclique : {ident}')
                seen.add(ident)
                graphic = self.graphic(ident)
                ident = activations[ident].get('owner') or (graphic.get('container') if graphic is not None else None)
            if ident not in participants:
                raise ConversionError(f'Participant de séquence introuvable : {ident!r}')
            return self.ids[ident]
        for ident in sorted(participants, key=lambda i: self.number(i, 'x')):
            item = participants[ident]
            lifeline = item.find('lifeline')
            name = item.get('name', '')
            if lifeline is not None and lifeline.get('classifier-name'):
                name += (': ' if name else '') + lifeline.get('classifier-name', '')
            keyword = 'actor' if item.get('type') == 'actor' else 'participant'
            self.lines.append(f'{keyword} "{literal(name or "Lifeline")}" as {self.ids[ident]}')
        events = []
        order = 0
        for ident, item in messages.items():
            source, target = self.endpoints(ident, item)
            sender, receiver = owner(source), owner(target)
            action = item.find('sequence-action')
            if action is None:
                raise ConversionError(f'sequence-action absente : {ident}')
            typ = action.get('type', 'call').lower()
            arrow = '-->' if typ in ('return', 'reply') else '->>' if _true(action, 'is-asynchronous') else '->'
            label = action.get('message', '') or item.get('name', '')
            args = [p.get('value', p.get('name', '')) for p in action.findall('./parameters/parameter')]
            if args:
                label += '(' + ', '.join(args) + ')'
            if action.get('assign-to-variable'):
                label = action.get('assign-to-variable', '') + ' = ' + label
            if action.get('return-value'):
                label += ' : ' + action.get('return-value', '')
            lines = []
            if typ == 'create':
                lines.append(f'create {receiver}')
            lines.append(f'{sender} {arrow} {receiver} : {literal(label)}')
            if typ in ('destroy', 'delete'):
                lines.append(f'destroy {receiver}')
            if typ not in ('call', 'create', 'destroy', 'delete', 'return', 'reply', 'send'):
                self.warn(f'Message {ident} : type {typ} représenté par un appel')
            graphic = self.graphic(ident)
            if graphic is None or graphic.find('layout') is None:
                self.warn('Ordre des messages sans coordonnées : ordre des définitions XML utilisé')
            events.append((self.number(ident, 'y1', self.number(ident, 'y', order)), 3, order, lines))
            order += 1
        for ident in activations:
            participant = owner(ident)
            y = self.number(ident, 'y')
            height = self.number(ident, 'height')
            if height <= 0:
                self.warn(f'Activation {ident} sans durée graphique : conservée en commentaire XML')
                continue
            events.append((y, 2, order, [f'activate {participant}']))
            events.append((y + height, 0, -order, [f'deactivate {participant}']))
            order += 1
        intervals = []
        for ident, item in view.elements.items():
            if item.get('type') != 'fragment':
                continue
            fragment = item.find('fragment')
            operator = fragment.get('operator', 'group') if fragment is not None else 'group'
            if operator not in ('alt', 'opt', 'loop', 'par', 'break', 'critical', 'group'):
                self.warn(f'Fragment {ident} : opérateur {operator} représenté par un groupe')
                operator = 'group'
            y = self.number(ident, 'y')
            end = y + self.number(ident, 'height')
            if end <= y:
                self.warn(f'Fragment {ident} sans intervalle : conservé en note')
                self.lines.append(f'note over {", ".join(self.ids[p] for p in list(participants)[:2])} : {literal(item.get("name", "") + " [" + operator + "]")}')
                continue
            intervals.append((y, end, ident, operator, item))
        # Un intervalle croisé ne peut être traduit en blocs PlantUML imbriqués.
        for y, end, ident, operator, item in intervals:
            if any(y < a < end < b or a < y < b < end for a, b, *_ in intervals):
                self.warn(f'Fragment {ident} croisé : bornes remplacées par des séparateurs')
                events.append((y, 1, order, [f'== {literal(operator + " " + item.get("name", ""))} ==']))
                continue
            operands = []
            if item.find('fragment') is not None:
                for ref in item.findall('./fragment/operands/operand'):
                    target = ref.get('uid') or ref.get('ref')
                    if target not in view.elements:
                        raise ConversionError(f'Opérande de fragment introuvable : {target}')
                    operands.append(target)
            operands.sort(key=lambda i: self.number(i, 'y'))
            def guard(ref):
                operand = view.elements[ref].find('interaction-operand')
                return operand.get('guard', '') if operand is not None else ''
            label = guard(operands[0]) if operands else item.get('name', '')
            events.append((y, 1, -end, [f'{operator} {literal(label)}']))
            events.append((end, 0, -y, ['end']))
            for operand in operands[1:]:
                events.append((self.number(operand, 'y'), 1, order, [f'else {literal(guard(operand))}']))
            order += 1
        # PlantUML ancre les activations sur les messages, pas sur des ordonnées.
        # Différer l'ouverture jusqu'au prochain message évite une réactivation
        # au même ancrage que la fermeture précédente (syntaxe refusée par PUML).
        pending: dict[str, int] = {}
        active: dict[str, int] = {}
        for _, _, _, lines in sorted(events, key=lambda e: e[:3]):
            if len(lines) == 1 and lines[0].startswith('activate '):
                participant = lines[0].split()[1]
                pending[participant] = pending.get(participant, 0) + 1
            elif len(lines) == 1 and lines[0].startswith('deactivate '):
                participant = lines[0].split()[1]
                if pending.get(participant, 0):
                    pending[participant] -= 1
                    self.warn('Activations sans message intermédiaire non dessinées ; XML conservé')
                elif active.get(participant, 0):
                    active[participant] -= 1
                    if not active[participant]:
                        self.lines.extend(lines)
            else:
                self.lines.extend(lines)
                if any(' ->' in line or ' -->' in line for line in lines):
                    for participant, count in pending.items():
                        if not count:
                            continue
                        if active.get(participant, 0):
                            self.warn('Activations imbriquées sur une même ligne de vie réunies ; XML conservé')
                        else:
                            self.lines.append(f'activate {participant}')
                        active[participant] = active.get(participant, 0) + count
                    pending.clear()
        known = set(messages) | set(activations) | set(participants)
        for ident, item in view.elements.items():
            if ident in known or item.get('type') in ('fragment', 'interaction-operand', 'property'):
                continue
            self.warn(f'Élément de séquence {ident} ({item.get("type")}) représenté en note')
            label = item.findtext('comment') or item.get('name') or item.get('type', '')
            self.lines.append(f'note as {self.ids[ident]}')
            self.lines.append(literal(label))
            self.lines.append('end note')

    def finish(self) -> str:
        # JSON échappe les retours de ligne : aucune donnée n'est une directive.
        self.lines.append("' simp2puml-source: éléments XML et occurrences sélectionnés ; archive, sans import inverse")
        self.lines.append("' simp2puml-context " + _archive_json(self.view.context))
        for ident, item in self.view.elements.items():
            record = {'id': ident, 'xml': ET.tostring(item, encoding='unicode'),
                      'graphics': [ET.tostring(g, encoding='unicode') for g in self.view.graphics.get(ident, [])]}
            self.lines.append("' simp2puml-source " + _archive_json(record))
        self.lines.append('@enduml')
        return '\n'.join(self.lines) + '\n'


def write_view(view: SIMPView) -> tuple[str, list[str]]:
    writer = _Writer(view)
    if view.kind == 'uml-sequence':
        writer.sequence()
    else:
        writer.structural()
    if view.graphics:
        writer.warn('Mise en page et styles SIMP recalculés par PlantUML ; XML des occurrences conservé en commentaires')
    return writer.finish(), writer.warnings
