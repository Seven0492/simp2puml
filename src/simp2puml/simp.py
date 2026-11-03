"""Lecture SIMP monofichier et écriture du profil de classes XML 12.75.

Les vues enrichies et les autres familles passent par simp_document, puis
simp_puml à l'export. Le sous-ensemble de classes historique conserve son API.

Les définitions se trouvent dans items et models//abstract-items, les occurrences
visuelles dans layer. L'absence de visibilité est conservée en omettant
l'attribut XML : c'est une hypothèse non démontrée par la fixture. Les types
UML observés et les noms de classes non ambigus reçoivent un type-id ; les
autres restent textuels, sans type-id. Leur interprétation par l'outil SIMP
n'a pas été vérifiée. Le fichier produit ne garantit pas sa compatibilité avec
l'application ; seule la forme observée sert de profil.
"""
import uuid
import xml.etree.ElementTree as ET
from typing import NoReturn

from .model import (ConversionError, UMLAttribute, UMLClass, UMLDiagram,
                   UMLOperation, UMLParameter, UMLRelationship, UMLNote)
from .simp_document import (list_diagrams, needs_extended_reader, read_project,
                            read_view, select_diagram)

_VIS = {'public': '+', 'private': '-', 'protected': '#', 'package': '~'}
_REV_VIS = {v: k for k, v in _VIS.items()}
_BUILTINS = {'Integer', 'Boolean', 'String', 'Real'}


def _fail(message) -> NoReturn:
    raise ConversionError(message)


def _visibility(element):
    value = element.get('visibility')
    if value in (None, ''):
        return None
    if value not in _VIS:
        _fail(f'Visibilité SIMP inconnue : {value}')
    return _VIS[value]


def _type(element, index):
    value = element.get('type')
    ref = element.get('type-id', '')
    if ref.startswith('Class:'):
        target = index.get(ref[6:])
        if target is None:
            _fail(f'Type référencé introuvable : {ref}')
        if value:
            return value
        if not target.get('name'):
            _fail(f'Type référencé sans nom : {ref}')
        return target.get('name')
    if value:
        return value
    if not ref:
        return None
    if ref.startswith('Uml.') and len(ref) > 4:
        return ref[4:]
    _fail(f'Type référencé non résolu : {ref}')


def _truth(element, *names):
    return any(element.get(name, '').lower() in ('true', '1') for name in names)


def parse_simp(text: str, diagram: str | None = None) -> UMLDiagram:
    """Lit les définitions sélectionnées et les objets globaux présentés."""
    project, all_diagrams = read_project(text)
    selected_view = select_diagram(all_diagrams, diagram)
    if needs_extended_reader(project, selected_view):
        view = read_view(project, selected_view)
        return UMLDiagram(name=view.name, warnings=list(view.warnings), source_view=view)
    root, selected = project, selected_view
    abstract = selected.find('abstract-items')
    if abstract is None:
        _fail('abstract-items absent')
    items = root.findall('./items/item') + root.findall('./models//abstract-items/item')
    index = {}
    for item in items:
        ident = item.get('id')
        if not ident or ident in index:
            _fail(f'Identifiant d’élément absent ou dupliqué : {ident!r}')
        index[ident] = item
    seen = set(index)
    for item in items:
        for node in item.iter():
            if node.tag in ('attribute', 'operation', 'parameter', 'enumeration-item') and node.get('id'):
                if node.get('id') in seen:
                    _fail(f'Identifiant dupliqué : {node.get("id")}')
                seen.add(node.get('id'))
    selected_items = list(abstract)
    selected_ids = {x.get('id') for x in selected_items}
    shared_items = root.findall('./items/item') + root.findall('./models//model/abstract-items/item')
    shared_ids = {x.get('id') for x in shared_items}
    for graphic in selected.findall('./layer/item'):
        ident = graphic.get('id')
        if ident not in index:
            _fail(f'Objet graphique référencé introuvable : {ident!r}')
        if ident in shared_ids and ident not in selected_ids:
            selected_items.append(index[ident])
            selected_ids.add(ident)
    result = UMLDiagram(name=selected.get('name') or 'Diagram')
    kinds = {'class': 'class', 'uml-interface-classifier': 'interface',
             'enumeration': 'enumeration', 'uml-interface': 'interface_symbol'}
    classes = {}
    consumed_properties = set()
    name_counts = {}
    for item in selected_items:
        if item.get('type') in kinds:
            name_counts[item.get('name')] = name_counts.get(item.get('name'), 0) + 1

    def member_type(node, context):
        value = _type(node, index)
        ref = node.get('type-id', '')
        if ref.startswith('Class:') and name_counts.get(value, 0) > 1:
            result.warnings.append(f'Type {context} : référence {ref} perdue, nom ambigu {value!r}')
        return value

    for item in selected_items:
        typ = item.get('type')
        if typ not in kinds:
            continue
        cls = UMLClass(id=item.get('id', ''), name=item.get('name', ''),
                       kind=kinds[typ], is_abstract=_truth(item, 'is-abstract', 'abstract'),
                       literals=[x.get('name', '') for x in item.findall('./enumeration-items/enumeration-item')])
        classes[cls.id] = cls
        result.classes.append(cls)
        for attribute in item.findall('./attributes/attribute'):
            ref = attribute.get('property-ref')
            prop = index.get(ref)
            if not ref or prop is None or prop.get('type') != 'property':
                _fail(f'property-ref introuvable : {ref!r}')
            consumed_properties.add(ref)
            feature = prop.find('structural-feature')
            if feature is None:
                _fail(f'structural-feature absent pour {ref}')
            nodes = [attribute, prop, *prop.findall('feature'), feature]
            if any(_truth(n, 'abstract', 'is-abstract') for n in nodes):
                result.warnings.append(f'Attribut abstrait non représentable : {cls.name}.{attribute.get("name")}')
            cls.attributes.append(UMLAttribute(attribute.get('name', ''), member_type(feature, f'{cls.name}.{attribute.get("name")}'),
                _visibility(prop), any(_truth(n, 'static', 'is-static') for n in nodes), feature.get('multiplicity') or None))
        for op in item.findall('./operations/operation'):
            context = f'{cls.name}.{op.get("name", "")}'
            params = [UMLParameter(p.get('name', ''), member_type(p, f'{context}/{p.get("name", "")}'), p.get('multiplicity') or None)
                      for p in op.findall('./parameters/parameter')]
            cls.operations.append(UMLOperation(op.get('name', ''), params, member_type(op, context), _visibility(op),
                _truth(op, 'static', 'is-static'), _truth(op, 'abstract', 'is-abstract'), op.get('multiplicity') or None))
    for item in selected_items:
        if item.get('id') not in classes:
            continue
        missing = []
        for ref in dict.fromkeys(x.get('ref') for x in item.findall('./owned-items/owned-item')):
            prop = index.get(ref)
            if prop is None:
                _fail(f'owned-item référencé introuvable : {ref!r}')
            if prop.get('type') == 'property' and ref not in consumed_properties:
                missing.append(f'{prop.get("name", "")} ({ref})')
        if missing:
            result.warnings.append(f'Propriétés sans attribute ambiguës pour {item.get("name")} : ' + ', '.join(missing))
    notes = {}
    for item in selected_items:
        if item.get('type') == 'comment':
            note = UMLNote(item.get('id', ''), item.findtext('comment') or '')
            notes[note.id] = note
            result.notes.append(note)
    for item in selected_items:
        typ = item.get('type')
        if typ == 'comment-connector':
            source, target = item.get('from'), item.get('to')
            for endpoint in (source, target):
                if endpoint and endpoint not in index:
                    _fail(f'Extrémité de commentaire introuvable : {endpoint!r}')
            if not source or not target:
                _fail(f'Extrémité de commentaire introuvable : {source!r} → {target!r}')
            if source in notes and target in classes:
                notes[source].targets.append(target)
            elif target in notes and source in classes:
                notes[target].targets.append(source)
            else:
                result.warnings.append(f'Connecteur de commentaire non représentable : {item.get("id")} {source} → {target}')
            continue
        if typ not in ('generalization', 'relationship', 'uml-provided-interface-connector'):
            continue
        relation = item.find('relation') if typ == 'relationship' else item
        if relation is None:
            _fail(f'Relation sans définition : {item.get("id")}')
        source, target = relation.get('from'), relation.get('to')
        for endpoint in (source, target):
            if endpoint and endpoint not in index:
                _fail(f'Extrémité de relation introuvable : {endpoint!r} ({item.get("id")})')
        if not source or not target:
            ident = item.get('id')
            referenced = any(value == ident for node in root.iter() for key, value in node.attrib.items()
                             if node is not item or key != 'id')
            if typ == 'generalization' and bool(source) != bool(target) and selected.find('./layer/item') is not None and not referenced:
                missing = 'from' if not source else 'to'
                present = 'cible' if not source else 'source'
                name = index[source or target].get('name') or (source or target)
                result.warnings.append(f'Généralisation {ident} orpheline et non représentée omise : extrémité {missing} manquante, {present} {name}')
                continue
            _fail(f'Extrémité de relation introuvable : {source!r} → {target!r} ({ident})')
        if source not in classes or target not in classes:
            result.warnings.append(f'Relation vers élément omis : {source} → {target}')
            continue
        kind = {'generalization': 'inheritance', 'relationship': 'association', 'uml-provided-interface-connector': 'provided_interface'}[typ]
        roles = [relation.find('start-role'), relation.find('end-role')] if typ == 'relationship' else [None, None]
        if typ == 'relationship':
            diamonds = [(i, role.get('association-type', 'association')) for i, role in enumerate(roles)
                        if role is not None and role.get('association-type', 'association') not in ('', 'association')]
            if len(diamonds) > 1 or any(k not in ('aggregation', 'composition') for _, k in diamonds):
                _fail(f'Types d’extrémité non représentables : {item.get("id")}')
            if diamonds:
                side, kind = diamonds[0]
                if side == 1:
                    source, target = target, source
                    roles.reverse()
            if _truth(relation, 'derived'):
                result.warnings.append(f'Relation dérivée non représentée : {item.get("id")}')
            for role in roles:
                if role is not None:
                    for field in ('navigability', 'attribute-ref'):
                        if role.get(field):
                            result.warnings.append(f'Relation {item.get("id")} : {field} non représenté : {role.get(field)}')
        def role_value(i, field):
            return roles[i].get(field) or None if roles[i] is not None else None
        result.relationships.append(UMLRelationship(source, target, kind, item.get('name') or None,
            role_value(0, 'multiplicity'), role_value(1, 'multiplicity'), role_value(0, 'name'), role_value(1, 'name'),
            _visibility(roles[0]) if roles[0] is not None else None, _visibility(roles[1]) if roles[1] is not None else None))
    known = set(kinds) | {'property', 'comment', 'comment-connector', 'generalization', 'relationship', 'uml-provided-interface-connector'}
    for item in selected_items:
        if item.get('type') not in known:
            result.warnings.append(f'Élément omis : type={item.get("type", "")} id={item.get("id", "")} nom={item.get("name", "")}')
    result.validate()
    return result


def _new_id(prefix=''):
    return prefix + uuid.uuid4().hex


def _optional_type(element, value, class_types):
    if value is not None:
        if value == '':
            _fail('Type vide non représentable distinctement d’un type absent')
        element.set('type', value)
        if value in _BUILTINS:
            element.set('type-id', 'Uml.' + value)
        elif value in class_types:
            element.set('type-id', 'Class:' + class_types[value])


def _set_visibility(element, value):
    if value is not None:
        element.set('visibility', _REV_VIS[value])


def _class_size(cls):
    if cls.kind == 'interface_symbol':
        return 30, 30
    lines = [cls.name, *cls.literals]
    lines.extend(f'{a.name}: {a.type or ""}' for a in cls.attributes)
    lines.extend(f'{o.name}(' + ', '.join(f'{p.name}: {p.type or ""}' for p in o.parameters)
                 + f'): {o.return_type or ""}' for o in cls.operations)
    return max(200, 9 * max(map(len, lines)) + 30), max(120, 55 + 20 * (len(cls.attributes) + len(cls.operations) + len(cls.literals)))


def _edge_points(source, target):
    x, y, w, h = source
    tx, ty, tw, th = target
    if source == target:
        return [(x + w, y + h // 2), (x + w + 35, y + h // 2),
                (x + w + 35, y - 35), (x + w // 2, y - 35), (x + w // 2, y)]
    cx, cy = x + w / 2, y + h / 2
    dx, dy = tx + tw / 2 - cx, ty + th / 2 - cy
    def border(bx, by, bw, bh, vx, vy):
        scale = min(bw / (2 * abs(vx)) if vx else float('inf'),
                    bh / (2 * abs(vy)) if vy else float('inf'))
        return round(bx + bw / 2 + vx * scale, 2), round(by + bh / 2 + vy * scale, 2)
    return [border(x, y, w, h, dx, dy), border(tx, ty, tw, th, -dx, -dy)]


def write_simp(diagram: UMLDiagram) -> str:
    """Écrit le profil observé, sans garantie de compatibilité applicative."""
    if diagram.source_view is not None:
        _fail('La vue SIMP enrichie est exportable en PlantUML uniquement ; import inverse non pris en charge')
    diagram.validate()
    root = ET.Element('sim-project', {'version': '12.75', 'multi-file': 'no', 'uid': _new_id('simp')})
    ET.SubElement(root, 'meta')
    ET.SubElement(root, 'counters', {'models': '1'})
    ET.SubElement(root, 'alias-groups')
    types = ET.SubElement(root, 'types', {'default': 'UML'})
    ET.SubElement(ET.SubElement(types, 'type-sets'), 'type-set', {'file': 'UmlTypes.xml'})
    custom_types = ET.SubElement(types, 'custom-types')
    counts = {}
    for cls in diagram.classes:
        counts[cls.name] = counts.get(cls.name, 0) + 1
    class_types = {cls.name: cls.id for cls in diagram.classes if counts[cls.name] == 1 and cls.name not in _BUILTINS}
    used_types = {a.type for cls in diagram.classes for a in cls.attributes}
    used_types.update(op.return_type for cls in diagram.classes for op in cls.operations)
    used_types.update(p.type for cls in diagram.classes for op in cls.operations for p in op.parameters)
    for name, ident in class_types.items():
        if name in used_types:
            ET.SubElement(custom_types, 'type', {'id': 'Class:' + ident, 'tid': '', 'name': name, 'description': '', 'class-ref': ident})
    globals_ = ET.SubElement(root, 'items')
    models = ET.SubElement(root, 'models')
    model = ET.SubElement(models, 'model', {'id': _new_id('p'), 'name': 'Model1', 'namespace': '', 'order-index': '1'})
    ET.SubElement(model, 'abstract-items')
    ET.SubElement(model, 'sub-models')
    view = ET.SubElement(model, 'diagram', {'id': _new_id('p'), 'name': diagram.name, 'type': 'uml-class', 'order-index': '1'})
    abstract = ET.SubElement(view, 'abstract-items')
    layer = ET.SubElement(view, 'layer', {'id': _new_id(), 'name': 'Default', 'visible': 'true', 'enabled': 'true', 'locked': 'false'})
    entities = [*diagram.classes, *diagram.notes]
    iids = {obj.id: _new_id() for obj in entities}
    sizes = {cls.id: _class_size(cls) for cls in diagram.classes}
    for note in diagram.notes:
        lines = note.text.splitlines() or ['']
        width = min(500, max(200, max(map(len, lines)) * 7 + 30))
        columns = max(1, (width - 30) // 7)
        wrapped_lines = sum(max(1, (len(line) + columns - 1) // columns) for line in lines)
        sizes[note.id] = (width, max(120, 40 + 18 * wrapped_lines))
    cell_width = max((size[0] for size in sizes.values()), default=200) + 100
    cell_height = max((size[1] for size in sizes.values()), default=120) + 100
    layouts = {obj.id: ((n % 3) * cell_width, (n // 3) * cell_height, *sizes[obj.id]) for n, obj in enumerate(entities)}
    graphics = {}
    for n, cls in enumerate(diagram.classes):
        typ = {'class': 'class', 'interface': 'uml-interface-classifier', 'enumeration': 'enumeration', 'interface_symbol': 'uml-interface'}[cls.kind]
        item = ET.SubElement(globals_ if cls.kind == 'interface_symbol' else abstract, 'item',
                             {'id': cls.id, 'type': typ, 'name': cls.name, 'style-class': ''})
        if cls.kind != 'interface_symbol':
            item.set('visibility', 'package')
        if cls.is_abstract:
            item.set('is-abstract', 'true')
        if cls.kind in ('interface', 'enumeration'):
            stereotype = 'interface' if cls.kind == 'interface' else 'enumeration'
            ET.SubElement(ET.SubElement(item, 'stereotypes'), 'stereotype', {'name': stereotype})
            ET.SubElement(ET.SubElement(item, 'stereotype-instances'), 'stereotype', {'id': cls.id + '_' + stereotype + '_c', 'type': stereotype, 'ref': ''})
        if cls.kind == 'interface_symbol':
            ET.SubElement(item, 'interface')
        if cls.attributes:
            owned = ET.SubElement(item, 'owned-items')
            attributes = ET.SubElement(item, 'attributes')
            for attr in cls.attributes:
                prop_id = _new_id()
                ET.SubElement(owned, 'owned-item', {'ref': prop_id})
                ET.SubElement(attributes, 'attribute', {'id': _new_id('a'), 'name': attr.name, 'property-ref': prop_id})
                prop = ET.SubElement(globals_, 'item', {'id': prop_id, 'type': 'property', 'name': attr.name, 'owner': cls.id, 'style-class': ''})
                _set_visibility(prop, attr.visibility)
                feature = ET.SubElement(prop, 'feature')
                if attr.is_static:
                    feature.set('static', 'true')
                structural = ET.SubElement(prop, 'structural-feature')
                _optional_type(structural, attr.type, class_types)
                if attr.multiplicity is not None:
                    structural.set('multiplicity', attr.multiplicity)
                ET.SubElement(prop, 'property')
        if cls.operations:
            operations = ET.SubElement(item, 'operations')
            for op in cls.operations:
                node = ET.SubElement(operations, 'operation', {'id': _new_id('o'), 'name': op.name})
                _optional_type(node, op.return_type, class_types)
                _set_visibility(node, op.visibility)
                if op.is_static: node.set('static', 'true')
                if op.is_abstract: node.set('abstract', 'true')
                if op.multiplicity is not None: node.set('multiplicity', op.multiplicity)
                if op.parameters:
                    params = ET.SubElement(node, 'parameters')
                    for param in op.parameters:
                        p = ET.SubElement(params, 'parameter', {'id': _new_id('p'), 'name': param.name})
                        _optional_type(p, param.type, class_types)
                        if param.multiplicity is not None: p.set('multiplicity', param.multiplicity)
        if cls.literals:
            literals = ET.SubElement(item, 'enumeration-items')
            for value in cls.literals:
                ET.SubElement(literals, 'enumeration-item', {'id': _new_id('a'), 'name': value})
        graphic = ET.SubElement(layer, 'item', {'id': cls.id, 'iid': iids[cls.id], 'type': 'entity', 'order-index': str(n)})
        graphics[cls.id] = graphic
        x, y, width, height = layouts[cls.id]
        ET.SubElement(graphic, 'layout', {'x': str(x), 'y': str(y), 'width': str(width), 'height': str(height), 'ax': str(x), 'ay': str(y), 'awidth': str(width), 'aheight': str(height)})
        ET.SubElement(graphic, {'enumeration': 'enumeration', 'interface_symbol': 'interface'}.get(cls.kind, 'class'))
    for n, note in enumerate(diagram.notes, len(diagram.classes)):
        item = ET.SubElement(globals_, 'item', {'id': note.id, 'type': 'comment', 'name': 'Comment', 'style-class': ''})
        ET.SubElement(item, 'comment').text = note.text
        graphic = ET.SubElement(layer, 'item', {'id': note.id, 'iid': iids[note.id], 'type': 'entity', 'order-index': str(n)})
        x, y, width, height = layouts[note.id]
        ET.SubElement(graphic, 'layout', {'x': str(x), 'y': str(y), 'width': str(width), 'height': str(height), 'ax': str(x), 'ay': str(y), 'awidth': str(width), 'aheight': str(height)})
    def edge(ident, source, target, typ, attached=False):
        iid = _new_id()
        graphic = ET.SubElement(layer, 'item', {'id': ident, 'iid': iid, 'type': typ, 'order-index': str(len(layer))})
        points = _edge_points(layouts[source], layouts[target])
        (x1, y1), (x2, y2) = points[0], points[-1]
        layout = ET.SubElement(graphic, 'layout', {'line-style': 'oblique', 'x1': str(x1), 'y1': str(y1), 'x2': str(x2), 'y2': str(y2)})
        path = ET.SubElement(layout, 'points')
        for x, y in points:
            ET.SubElement(path, 'point', {'x': str(x), 'y': str(y)})
        if attached:
            ET.SubElement(ET.SubElement(graphic, 'attached-parts'), 'attached-part', {'ref': iids[target]})
            parts = graphics[target].find('attached-parts')
            if parts is None:
                parts = ET.SubElement(graphics[target], 'attached-parts')
            ET.SubElement(parts, 'attached-part', {'ref': iid})
        ET.SubElement(graphic, 'relation', {'from': source, 'from-iid': iids[source], 'to': target, 'to-iid': iids[target]})
    for rel in diagram.relationships:
        ident = _new_id()
        typ = 'generalization' if rel.kind == 'inheritance' else 'uml-provided-interface-connector' if rel.kind == 'provided_interface' else 'relationship'
        attrs = {'id': ident, 'type': typ, 'name': rel.name or '', 'style-class': '', 'from': rel.source, 'to': rel.target}
        if typ != 'uml-provided-interface-connector': attrs['visibility'] = 'package'
        item = ET.SubElement(globals_ if typ == 'uml-provided-interface-connector' else abstract, 'item', attrs)
        if typ == 'relationship':
            node = ET.SubElement(item, 'relation', {'from': rel.source, 'to': rel.target, 'derived': 'false'})
            for i, tag in enumerate(('start-role', 'end-role')):
                role = ET.SubElement(node, tag, {'id': _new_id('umlar-'), 'name': (rel.source_role if i == 0 else rel.target_role) or '', 'navigability': '', 'association-type': rel.kind if i == 0 and rel.kind in ('aggregation', 'composition') else 'association'})
                mult = rel.source_multiplicity if i == 0 else rel.target_multiplicity
                if mult is not None: role.set('multiplicity', mult)
                _set_visibility(role, rel.source_visibility if i == 0 else rel.target_visibility)
        elif any((rel.source_multiplicity, rel.target_multiplicity, rel.source_role, rel.target_role, rel.source_visibility, rel.target_visibility)):
            _fail(f'Détails de relation non représentables : {rel.kind}')
        edge(ident, rel.source, rel.target, 'relation', typ == 'uml-provided-interface-connector')
    for note in diagram.notes:
        for target in note.targets:
            ident = _new_id('di')
            ET.SubElement(globals_, 'item', {'id': ident, 'type': 'comment-connector', 'name': '', 'style-class': '', 'from': note.id, 'to': target})
            edge(ident, note.id, target, 'comment-connector')
    text = '<?xml version="1.0" encoding="utf-8"?>\n' + ET.tostring(root, encoding='unicode')
    try:
        ET.fromstring(text)
    except (ET.ParseError, ValueError) as exc:
        raise ConversionError(f'Donnée non représentable en XML : {exc}') from exc
    return text
