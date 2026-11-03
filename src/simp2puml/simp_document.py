"""Lecture structurelle SIMP, indépendante des notations et sans I/O externe.

Le XML source reste attaché à la vue : les adaptateurs peuvent diagnostiquer
les constructions sans équivalent plutôt que les supprimer silencieusement.
Les occurrences graphiques (iid) sont distinctes des identités sémantiques.
"""
from dataclasses import dataclass, field
import re
import xml.etree.ElementTree as ET

from .model import ConversionError


@dataclass
class SIMPView:
    name: str
    kind: str
    elements: dict[str, ET.Element]
    graphics: dict[str, list[ET.Element]] = field(default_factory=dict)
    warnings: list[str] = field(default_factory=list)
    available_ids: set[str] = field(default_factory=set)
    containers: dict[str, str] = field(default_factory=dict)
    context: dict = field(default_factory=dict)
    omitted_relations: set[str] = field(default_factory=set)

    @property
    def relationship_count(self) -> int:
        total = 0
        for ident, item in self.elements.items():
            candidates = [item, *item.findall('relation'), *item.findall('sequence-action')]
            for graphic in self.graphics.get(ident, []):
                candidates.extend(graphic.findall('relation') + graphic.findall('sequence-action'))
            total += any(n.get('from') or n.get('to') for n in candidates)
        return total


def read_project(text: str) -> tuple[ET.Element, list[ET.Element]]:
    if re.search(r'<!\s*(?:DOCTYPE|ENTITY)\b', text, re.IGNORECASE):
        raise ConversionError('DTD et ENTITY interdites')
    try:
        root = ET.fromstring(text)
    except (ET.ParseError, ValueError) as exc:
        raise ConversionError(f'XML invalide : {exc}') from exc
    if root.tag != 'sim-project' or root.get('multi-file') != 'no':
        raise ConversionError('Projet SIMP monofichier attendu (multi-file="no")')
    if not re.fullmatch(r'\d+(?:\.\d+)?', root.get('version', '')):
        raise ConversionError('Version de format SIMP absente ou invalide')
    stack = [(root, 0)]
    while stack:
        node, depth = stack.pop()
        if depth > 100:
            raise ConversionError('Imbrication XML supérieure à 100')
        stack.extend((child, depth + 1) for child in node)
    diagrams = root.findall('./models//model/diagram')
    if not diagrams:
        raise ConversionError('Aucun diagramme SIMP disponible')
    identities = [d.get('id') for d in diagrams]
    if any(not ident for ident in identities) or len(set(identities)) != len(identities):
        raise ConversionError('Identifiant de diagramme absent ou dupliqué')
    return root, diagrams


def list_diagrams(text: str) -> list[tuple[str, str, str]]:
    """Identifiant, nom et type de chaque diagramme, dans l'ordre du projet."""
    _, diagrams = read_project(text)
    return [(d.get('id', ''), d.get('name', ''), d.get('type', '')) for d in diagrams]


def select_diagram(diagrams: list[ET.Element], selector: str | None) -> ET.Element:
    choices = ', '.join(f'{d.get("name", "")} ({d.get("id", "")}, {d.get("type", "")})'
                        for d in diagrams)
    if selector is None and len(diagrams) == 1:
        return diagrams[0]
    if selector is None:
        raise ConversionError(f'Sélection de diagramme requise ; choix : {choices}')
    matches = [d for d in diagrams if selector in (d.get('id'), d.get('name'))]
    if len(matches) != 1:
        raise ConversionError(f'Diagramme absent ou ambigu : {selector} ; choix : {choices}')
    return matches[0]


def read_view(root: ET.Element, selected: ET.Element) -> SIMPView:
    abstract = selected.find('abstract-items')
    if abstract is None:
        raise ConversionError('abstract-items absent')
    items = root.findall('./items/item') + root.findall('./models//abstract-items/item')
    index: dict[str, ET.Element] = {}
    identities: set[str] = set()
    for item in items:
        ident = item.get('id', '')
        if not ident or ident in index:
            raise ConversionError(f'Identifiant d’élément absent ou dupliqué : {ident!r}')
        index[ident] = item
    identities.update(index)
    for item in items:
        for node in item.iter():
            ident = node.get('id') or node.get('uid')
            if node is not item and node.tag in ('attribute', 'operation', 'parameter', 'enumeration-item') and ident:
                if ident in identities:
                    raise ConversionError(f'Identifiant dupliqué : {ident}')
                identities.add(ident)
    shared = {x.get('id') for x in root.findall('./items/item') + root.findall('./models//model/abstract-items/item')}
    elements = {x.get('id', ''): x for x in abstract.findall('item')}
    graphics: dict[str, list[ET.Element]] = {}
    warnings = []
    containers = {}
    iids = set()
    for layer in selected.findall('layer'):
        parent_of = {child: parent for parent in layer.iter() for child in parent}
        for graphic in layer.findall('.//item'):
            ident = graphic.get('id', '')
            if ident not in index:
                raise ConversionError(f'Objet graphique référencé introuvable : {ident!r}')
            if ident not in elements and ident not in shared:
                warnings.append(f'Occurrence hors diagramme sélectionné exclue : {ident}')
                continue
            elements[ident] = index[ident]
            graphics.setdefault(ident, []).append(graphic)
            iid = graphic.get('iid')
            if iid:
                if iid in iids:
                    raise ConversionError(f'Identifiant d’occurrence dupliqué : {iid}')
                iids.add(iid)
            ancestor = parent_of.get(graphic)
            while ancestor is not None and ancestor.tag != 'item':
                ancestor = parent_of.get(ancestor)
            parent = graphic.get('container') or (ancestor.get('id') if ancestor is not None else None)
            if parent:
                containers[ident] = parent
        if layer.get('visible') == 'false':
            warnings.append(f'Calque masqué inclus : {layer.get("name", "")}')
    # Propriétés des attributs et propriétaires d'activations sont des définitions
    # auxiliaires, pas des classes à importer depuis une autre vue.
    pending = list(elements.values())
    for item in pending:
        for attr in item.findall('./attributes/attribute'):
            ref = attr.get('property-ref')
            if ref:
                if ref not in index or index[ref].get('type') != 'property':
                    raise ConversionError(f'property-ref introuvable : {ref!r}')
                elements[ref] = index[ref]
    context = {'version': root.get('version'), 'diagram': dict(selected.attrib),
               'metadata': [ET.tostring(n, encoding='unicode') for n in selected if n.tag not in ('abstract-items', 'layer')],
               'types': ET.tostring(root.find('types'), encoding='unicode') if root.find('types') is not None else None}
    view = SIMPView(selected.get('name') or 'Diagram', selected.get('type') or 'unknown', elements, graphics, warnings, set(index), containers, context)
    for ident, item in elements.items():
        if item.get('type') != 'generalization' or ident in graphics:
            continue
        source, target = item.get('from'), item.get('to')
        if bool(source) == bool(target) or (source or target) not in index or not graphics:
            continue
        referenced = any(value == ident for node in root.iter() for key, value in node.attrib.items()
                         if node is not item or key != 'id')
        if not referenced:
            view.omitted_relations.add(ident)
            missing = 'from' if not source else 'to'
            warnings.append(f'Généralisation {ident} orpheline et non représentée omise : extrémité {missing} manquante ; XML conservé')
    if root.get('version') not in ('10', '12.75'):
        warnings.append(f'Format SIMP {root.get("version")} lu structurellement, profil non validé')
    return view


_CLASS_TYPES = {'class', 'uml-interface-classifier', 'enumeration', 'uml-interface',
                'property', 'comment', 'comment-connector', 'generalization',
                'relationship', 'uml-provided-interface-connector'}


def needs_extended_reader(root: ET.Element, selected: ET.Element) -> bool:
    if selected.get('type') != 'uml-class' or root.get('version') != '12.75':
        return True
    # Les anciens modèles et les constructions enrichies passent par la vue
    # structurelle. Le sous-ensemble historique garde son API de dataclasses.
    view = read_view(root, selected)
    if any(c in selected.get('name', '') for c in "%\n\r'\"\\"):
        return True
    for item in view.elements.values():
        if any(c in item.get('name', '') for c in '%\n\r"\\') or '<img:' in item.get('name', '').lower():
            return True
        if item.get('type') not in _CLASS_TYPES:
            return True
        stereotypes = [s.get('name') for s in item.findall('./stereotypes/stereotype')]
        if stereotypes and stereotypes != [{'enumeration': 'enumeration', 'uml-interface-classifier': 'interface'}.get(item.get('type'))]:
            return True
        for node in item.iter():
            if node.tag == 'attribute' and not node.get('property-ref'):
                return True
            if any(node.get(key) not in (None, '', 'false', '0') for key in
                   ('default-value', 'initial-value', 'is-leaf', 'is-active', 'read-only', 'is-read-only')):
                return True
            if node.tag in ('start-role', 'end-role') and node.get('navigability'):
                return True
            if node.tag == 'relation' and node.get('derived', '').lower() in ('true', '1'):
                return True
            if node.tag in ('attribute', 'operation', 'parameter', 'enumeration-item') and not re.fullmatch(r'(?:[^\W\d]|_)\w*', node.get('name', '')):
                return True
    return False
