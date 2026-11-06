## Point d'entrée de la bibliothèque : texte PlantUML → SVG + diagnostics.
import std/[strutils]
import common, preprocess, compose, graphdiag, sequence, activity, treediag, datadiag, gantt

type RenderResult* = object
  svg*: string
  width*, height*: float
  kind*: DiagramKind
  diags*: seq[Diagnostic]
  regions*: seq[Region]
  blocks*: int

proc renderPuml*(text: string, blockIndex = 0): RenderResult =
  let ctx = newContext()
  let text = if text.startsWith("\xEF\xBB\xBF"): text[3 .. ^1] else: text
  let doc = prepare(text, ctx, blockIndex)
  result.blocks = max(1, findBlocks(text).len)
  var inner: Svg
  var w, h: float
  try:
    case doc.kind
    of dkClass, dkObject, dkUsecase, dkComponent, dkState:
      var g = parseGraph(doc, ctx)
      (inner, w, h) = renderGraph(g, ctx)
    of dkActivity:
      var legacy = false
      for l in doc.lines:
        let t = l.text.strip()
        if t.startsWith("(*)") or t.endsWith("(*)") or t.contains("(*top)"): legacy = true
      if legacy:
        var g = parseGraph(doc, ctx)
        (inner, w, h) = renderGraph(g, ctx)
      else:
        (inner, w, h) = renderActivity(doc, ctx)
    of dkSequence:
      (inner, w, h) = renderSequence(doc, ctx)
    of dkMindmap, dkWbs:
      (inner, w, h) = renderTree(doc, ctx)
    of dkJson, dkYaml:
      (inner, w, h) = renderData(doc, ctx)
    of dkGantt:
      (inner, w, h) = renderGantt(doc, ctx)
    of dkTiming:
      ctx.warn(doc.startLine, "diagramme de chronogramme : rendu natif non disponible, texte seulement")
      (inner, w, h) = placeholder(ctx, "Chronogramme (timing)", "Aperçu natif non disponible pour ce type")
    of dkUnsupported:
      ctx.warn(doc.startLine, "type @start" & doc.tag & " : aperçu natif non disponible")
      (inner, w, h) = placeholder(ctx, "@start" & doc.tag, "Aperçu natif non disponible pour ce type")
    of dkEmpty:
      (inner, w, h) = placeholder(ctx, "Diagramme vide", "Ajoutez des éléments entre @startuml et @enduml")
  except TooComplex as e:
    ctx.error(e.line, e.msg)
    (inner, w, h) = placeholder(ctx, "Diagramme trop imbriqué", e.msg)
  except Exception as e:
    # Exception couvre aussi les Defect (index, débordement) : un défaut du
    # moteur devient un diagnostic au lieu de tuer le processus de l'éditeur
    ctx.error(doc.startLine, "erreur interne du rendu : " & e.msg)
    (inner, w, h) = placeholder(ctx, "Erreur de rendu", e.msg)
  if w <= 0 or h <= 0:
    (inner, w, h) = placeholder(ctx, "Diagramme vide", "Aucun élément à afficher")
  result.svg = compose(ctx, doc.meta, inner, w, h)
  let vb = result.svg.find("viewBox=\"0 0 ")
  if vb >= 0:
    let parts = result.svg[vb + 13 ..< result.svg.find('"', vb + 13)].split(' ')
    result.width = parseFloat(parts[0])
    result.height = parseFloat(parts[1])
  result.kind = doc.kind
  result.diags = ctx.diags
  result.regions = ctx.regions
