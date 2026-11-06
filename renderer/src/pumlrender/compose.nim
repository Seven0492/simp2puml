## Habillage commun : titre, en-tête, pied de page, légende et légende de figure.
import std/[strutils]
import common

proc compose*(ctx: Context, meta: Meta, inner: Svg, w, h: float, regionStart = 0): string =
  let pad = 12.0
  var headerLines = if meta.header.len > 0: meta.header.split('\n') else: @[]
  var titleLines = if meta.title.len > 0: meta.title.split('\n') else: @[]
  var footerLines = if meta.footer.len > 0: meta.footer.split('\n') else: @[]
  var captionLines = if meta.caption.len > 0: meta.caption.split('\n') else: @[]
  var legendLines = if meta.legend.len > 0: splitLabel(meta.legend) else: @[]
  let headerH = if headerLines.len > 0: blockHeight(headerLines.len, 11) + 6 else: 0.0
  let titleH = if titleLines.len > 0: blockHeight(titleLines.len, 17) + 12 else: 0.0
  let legendW = if legendLines.len > 0: blockWidth(legendLines) + 20 else: 0.0
  let legendH = if legendLines.len > 0: blockHeight(legendLines.len) + 14 else: 0.0
  let captionH = if captionLines.len > 0: blockHeight(captionLines.len, 12) + 8 else: 0.0
  let footerH = if footerLines.len > 0: blockHeight(footerLines.len, 11) + 6 else: 0.0
  var W = max(w, 40.0)
  W = max(W, blockWidth(titleLines, 17, true) + 2 * pad)
  W = max(W, blockWidth(headerLines, 11) + 2 * pad)
  W = max(W, blockWidth(footerLines, 11) + 2 * pad)
  W = max(W, legendW + 2 * pad)
  W = max(W, blockWidth(captionLines, 12) + 2 * pad)
  let innerTop = headerH + titleH + (if headerH + titleH > 0: 4.0 else: 0.0)
  let offX = (W - w) / 2
  for i in regionStart ..< ctx.regions.len:
    ctx.regions[i].x += offX
    ctx.regions[i].y += innerTop
  for l in ctx.links.mitems:
    for p in l.points.mitems:
      p = (p[0] + offX, p[1] + innerTop)
  var svg: Svg
  svg.defs = inner.defs
  if headerLines.len > 0:
    svg.textBlock(W - pad, 4, headerLines, "end", 11, fill = Muted)
    let headerW = blockWidth(headerLines, 11)
    ctx.region(meta.headerLine, W - pad - headerW, 2, headerW, headerH)
  if titleLines.len > 0:
    svg.textBlock(W / 2, headerH + 6, titleLines, "middle", 17, bold = true)
    ctx.region(meta.titleLine, W / 2 - blockWidth(titleLines, 17, true) / 2, headerH + 6,
      blockWidth(titleLines, 17, true), blockHeight(titleLines.len, 17))
  svg.add "<g transform=\"translate(" & fmtF(offX) & " " & fmtF(innerTop) & ")\">\n"
  svg.add inner.body
  svg.add "</g>\n"
  var y = innerTop + h
  if legendLines.len > 0:
    y += 6
    let lx = case meta.legendAlign
      of "left": pad
      of "center": (W - legendW) / 2
      else: W - legendW - pad
    svg.rect(lx, y, legendW, legendH, HeaderFill, Muted, 4)
    svg.textBlock(lx + 10, y + 7, legendLines)
    ctx.region(meta.legendLine, lx, y, legendW, legendH)
    y += legendH + 4
  if captionLines.len > 0:
    svg.textBlock(W / 2, y + 4, captionLines, "middle", 12, italic = true, fill = Muted)
    let captionW = blockWidth(captionLines, 12)
    ctx.region(meta.captionLine, W / 2 - captionW / 2, y + 2, captionW, captionH)
    y += captionH
  if footerLines.len > 0:
    svg.textBlock(W / 2, y + 2, footerLines, "middle", 11, fill = Muted)
    let footerW = blockWidth(footerLines, 11)
    ctx.region(meta.footerLine, W / 2 - footerW / 2, y, footerW, footerH)
    y += footerH
  svg.finish(W, max(y, 20.0))

proc placeholder*(ctx: Context, title, message: string): (Svg, float, float) =
  var svg: Svg
  let lines = @[title, message]
  let w = max(blockWidth(lines, 13, true) + 40, 240.0)
  svg.rect(10, 10, w, 64, HeaderFill, Muted, 6, 1.2, "5 4")
  svg.text(10 + w / 2, 36, title, "middle", 13, bold = true)
  svg.text(10 + w / 2, 56, message, "middle", 12, fill = Muted)
  (svg, w + 20, 84.0)
