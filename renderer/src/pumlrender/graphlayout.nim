## Mise en page hiérarchique (Sugiyama simplifié) : rupture des cycles,
## couches par plus long chemin, nœuds factices pour les longues arêtes,
## ordonnancement par barycentres et placement horizontal par médianes.
import std/[algorithm, math, sequtils]

type
  LNode* = object
    w*, h*: float
    x*, y*: float          ## coin supérieur gauche (résultat)
  LEdge* = object
    u*, v*: int
    minlen*: int           ## 0 : même rang souhaité ; 1 : rang inférieur…
    points*: seq[(float, float)]  ## points intermédiaires de u vers v (résultat)
  Graph* = object
    nodes*: seq[LNode]
    edges*: seq[LEdge]
    hgap*, vgap*: float
    leftToRight*: bool
    width*, height*: float

proc initGraph*(hgap = 40.0, vgap = 60.0): Graph = Graph(hgap: hgap, vgap: vgap)

proc addNode*(g: var Graph, w, h: float): int =
  g.nodes.add LNode(w: w, h: h)
  g.nodes.high

proc addEdge*(g: var Graph, u, v: int, minlen = 1) =
  g.edges.add LEdge(u: u, v: v, minlen: minlen)

proc crossings(layers: seq[seq[int]], pos: seq[int], succ: seq[seq[int]]): int =
  for r in 0 ..< layers.high:
    var pairs: seq[(int, int)]
    for a in layers[r]:
      for b in succ[a]: pairs.add (pos[a], pos[b])
    for i in 0 ..< pairs.len:
      for j in i + 1 ..< pairs.len:
        if (pairs[i][0] - pairs[j][0]) * (pairs[i][1] - pairs[j][1]) < 0: inc result

proc layout*(g: var Graph) =
  let n = g.nodes.len
  if n == 0:
    g.width = 0
    g.height = 0
    return
  var W = newSeq[float](n)
  var H = newSeq[float](n)
  for i, nd in g.nodes:
    if g.leftToRight: (W[i] = nd.h; H[i] = nd.w) else: (W[i] = nd.w; H[i] = nd.h)

  # --- rupture des cycles par DFS
  var adj = newSeq[seq[int]](n)   # indices d'arêtes sortantes
  for ei, e in g.edges:
    if e.u != e.v and e.u in 0 ..< n and e.v in 0 ..< n: adj[e.u].add ei
  var reversed = newSeq[bool](g.edges.len)
  var state = newSeq[int](n)  # 0 blanc, 1 gris, 2 noir
  for root in 0 ..< n:
    if state[root] != 0: continue
    var stack: seq[(int, int)] = @[(root, 0)]
    state[root] = 1
    while stack.len > 0:
      let (v, k) = stack[^1]
      if k < adj[v].len:
        stack[^1] = (v, k + 1)
        let ei = adj[v][k]
        let w = g.edges[ei].v
        if state[w] == 1: reversed[ei] = true
        elif state[w] == 0:
          state[w] = 1
          stack.add (w, 0)
      else:
        state[v] = 2
        discard stack.pop()

  # --- rangs par plus long chemin
  var dagSucc = newSeq[seq[(int, int)]](n)
  var dagPred = newSeq[seq[(int, int)]](n)
  var indeg = newSeq[int](n)
  var hasEdge = newSeq[bool](n)
  for ei, e in g.edges:
    if e.u == e.v or e.u notin 0 ..< n or e.v notin 0 ..< n: continue
    let (a, b) = if reversed[ei]: (e.v, e.u) else: (e.u, e.v)
    let ml = if reversed[ei]: max(e.minlen, 1) else: e.minlen
    dagSucc[a].add (b, ml)
    dagPred[b].add (a, ml)
    inc indeg[b]
    hasEdge[a] = true
    hasEdge[b] = true
  var order: seq[int]
  var q: seq[int]
  var deg = indeg
  for i in 0 ..< n:
    if deg[i] == 0 and hasEdge[i]: q.add i
  var qi = 0
  while qi < q.len:
    let v = q[qi]
    inc qi
    order.add v
    for (w, _) in dagSucc[v]:
      dec deg[w]
      if deg[w] == 0: q.add w
  var rank = newSeq[int](n)
  for v in order:
    for (w, ml) in dagSucc[v]:
      rank[w] = max(rank[w], rank[v] + ml)
  # rapprocher les sources de leurs successeurs
  for idx in countdown(order.high, 0):
    let v = order[idx]
    if dagPred[v].len == 0 and dagSucc[v].len > 0:
      var m = high(int)
      for (w, ml) in dagSucc[v]: m = min(m, rank[w] - ml)
      if m != high(int) and m > rank[v]: rank[v] = m
  var maxRank = 0
  for v in 0 ..< n:
    if hasEdge[v]: maxRank = max(maxRank, rank[v])
  # nœuds isolés : grille compacte
  var isolated: seq[int]
  for v in 0 ..< n:
    if not hasEdge[v]: isolated.add v
  if isolated.len > 0:
    let anyEdges = order.len > 0
    let perRow = max(3, int(ceil(sqrt(float(n)))))
    var counts = newSeq[int](maxRank + 1)
    for v in 0 ..< n:
      if hasEdge[v]: inc counts[rank[v]]
    var r = 0
    for v in isolated:
      while r < counts.len and counts[r] >= perRow: inc r
      if r >= counts.len:
        counts.add 0
      rank[v] = r
      inc counts[r]
    maxRank = counts.high
    discard anyEdges

  # --- nœuds factices
  var total = n
  var segSucc: seq[seq[int]] = newSeq[seq[int]](n)
  var segPred: seq[seq[int]] = newSeq[seq[int]](n)
  var chains = newSeq[seq[int]](g.edges.len)
  var nrank = rank
  var nW = W
  var nH = H
  for ei, e in g.edges:
    if e.u == e.v or e.u notin 0 ..< n or e.v notin 0 ..< n: continue
    var (a, b) = if reversed[ei]: (e.v, e.u) else: (e.u, e.v)
    if nrank[a] > nrank[b]: swap(a, b)
    var prev = a
    if nrank[b] - nrank[a] > 1:
      for r in nrank[a] + 1 ..< nrank[b]:
        let d = total
        inc total
        nrank.add r
        nW.add 10.0
        nH.add 10.0
        segSucc.add @[]
        segPred.add @[]
        segSucc[prev].add d
        segPred[d].add prev
        chains[ei].add d
        prev = d
    if nrank[a] != nrank[b]:
      segSucc[prev].add b
      segPred[b].add prev
    if reversed[ei] xor (rank[e.u] > rank[e.v]):
      chains[ei].reverse()

  # --- couches et ordonnancement
  var layers = newSeq[seq[int]](maxRank + 1)
  for v in 0 ..< total:
    if nrank[v] > maxRank:
      layers.setLen(nrank[v] + 1)
      maxRank = nrank[v]
    layers[nrank[v]].add v
  var pos = newSeq[int](total)
  for layer in layers:
    for i, v in layer: pos[v] = i
  var best = layers
  var bestCross = crossings(layers, pos, segSucc)
  for iter in 0 ..< 12:
    let down = iter mod 2 == 0
    let rng = if down: toSeq(1 .. layers.high) else: toSeq(countdown(layers.high - 1, 0))
    for r in rng:
      var keyed: seq[(float, int, int)]
      for i, v in layers[r]:
        let nb = if down: segPred[v] else: segSucc[v]
        var k = float(pos[v])
        if nb.len > 0:
          var s = 0.0
          for w in nb: s += float(pos[w])
          k = s / float(nb.len)
        keyed.add (k, i, v)
      keyed.sort()
      for i, kv in keyed:
        layers[r][i] = kv[2]
        pos[kv[2]] = i
    let c = crossings(layers, pos, segSucc)
    if c < bestCross:
      bestCross = c
      best = layers
  layers = best
  for layer in layers:
    for i, v in layer: pos[v] = i

  # --- placement horizontal
  var cx = newSeq[float](total)
  let hgap = g.hgap
  for layer in layers:
    var x = 0.0
    for v in layer:
      cx[v] = x + nW[v] / 2
      x += nW[v] + hgap
  proc pack(layer: seq[int], desired: seq[float], nW: seq[float], hgap: float): seq[float] =
    let m = layer.len
    var a = desired
    for i in 1 ..< m:
      let minX = a[i - 1] + (nW[layer[i - 1]] + nW[layer[i]]) / 2 + hgap
      if a[i] < minX: a[i] = minX
    var b = desired
    for i in countdown(m - 2, 0):
      let maxX = b[i + 1] - (nW[layer[i + 1]] + nW[layer[i]]) / 2 - hgap
      if b[i] > maxX: b[i] = maxX
    result = newSeq[float](m)
    for i in 0 ..< m: result[i] = (a[i] + b[i]) / 2
    for i in 1 ..< m:
      let minX = result[i - 1] + (nW[layer[i - 1]] + nW[layer[i]]) / 2 + hgap
      if result[i] < minX: result[i] = minX
  for iter in 0 ..< 8:
    let down = iter mod 2 == 0
    let rng = if down: toSeq(0 .. layers.high) else: toSeq(countdown(layers.high, 0))
    for r in rng:
      let layer = layers[r]
      if layer.len == 0: continue
      var desired = newSeq[float](layer.len)
      for i, v in layer:
        var nb = if down: segPred[v] else: segSucc[v]
        if iter >= 4:
          nb = segPred[v] & segSucc[v]
        if nb.len == 0:
          desired[i] = cx[v]
        else:
          var xs: seq[float]
          for w in nb: xs.add cx[w]
          xs.sort()
          desired[i] = if xs.len mod 2 == 1: xs[xs.len div 2] else: (xs[xs.len div 2 - 1] + xs[xs.len div 2]) / 2
      let placed = pack(layer, desired, nW, hgap)
      for i, v in layer: cx[v] = placed[i]
  var minX = Inf
  for v in 0 ..< total: minX = min(minX, cx[v] - nW[v] / 2)
  for v in 0 ..< total: cx[v] -= minX

  # --- placement vertical
  var layerH = newSeq[float](layers.len)
  for r, layer in layers:
    for v in layer: layerH[r] = max(layerH[r], nH[v])
  var top = newSeq[float](layers.len)
  var y = 0.0
  for r in 0 ..< layers.len:
    top[r] = y
    y += layerH[r] + (if r < layers.high: g.vgap else: 0)
  var cy = newSeq[float](total)
  for v in 0 ..< total: cy[v] = top[nrank[v]] + layerH[nrank[v]] / 2
  var width = 0.0
  for v in 0 ..< total: width = max(width, cx[v] + nW[v] / 2)

  # --- résultats (transposés si gauche à droite)
  for v in 0 ..< n:
    if g.leftToRight:
      g.nodes[v].x = cy[v] - H[v] / 2
      g.nodes[v].y = cx[v] - W[v] / 2
    else:
      g.nodes[v].x = cx[v] - W[v] / 2
      g.nodes[v].y = cy[v] - H[v] / 2
  for ei in 0 ..< g.edges.len:
    g.edges[ei].points = @[]
    for d in chains[ei]:
      g.edges[ei].points.add(if g.leftToRight: (cy[d], cx[d]) else: (cx[d], cy[d]))
  if g.leftToRight:
    g.width = y
    g.height = width
  else:
    g.width = width
    g.height = y

