//! Layered graph layout (Sugiyama, the way dagre does it for Mermaid), in
//! top-to-bottom space; flow.rs turns it for LR / BT / RL.
//!
//! 1. Cycles broken by reversing DFS back edges.
//! 2. Ranks: longest path, then nodes pulled toward the side with more
//!    edges. Every rank is doubled so each edge has a middle slot, where
//!    its label sits (dagre's trick).
//! 3. Long edges become chains of dummy nodes.
//! 4. Order in each layer: barycenter sweeps up and down, the best by
//!    crossings kept; a cluster's nodes stay together and sibling clusters
//!    keep one order in every layer.
//! 5. x: difference constraints (separation in a layer, and every member
//!    of a cluster on its side of everything ordered beside it), relaxed
//!    toward the mean of each node's neighbours. y: layer by layer, the gaps
//!    widened for cluster padding and titles.

#[derive(Clone, Debug)]
pub struct Node {
    pub w: f32,
    pub h: f32,
    /// Innermost cluster.
    pub cluster: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    /// Ranks spanned at least (`-->` 1, `--->` 2, …).
    pub minlen: usize,
    /// The label's box, (0, 0) for none.
    pub label: (f32, f32),
}

#[derive(Clone, Debug)]
pub struct Cluster {
    pub parent: Option<usize>,
    /// Room beyond the padding: top, right, bottom, left (a title's side).
    pub extra: [f32; 4],
    /// Smallest box (to fit the title).
    pub min: (f32, f32),
}

#[derive(Clone, Debug)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub clusters: Vec<Cluster>,
}

#[derive(Clone, Copy, Debug)]
pub struct Spacing {
    pub node: f32,
    pub rank: f32,
    pub edge: f32,
    pub pad: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Out {
    /// Node centres.
    pub nodes: Vec<(f32, f32)>,
    /// Each edge's points from its source's centre to its target's.
    pub edges: Vec<Vec<(f32, f32)>>,
    /// Each edge's label centre.
    pub labels: Vec<Option<(f32, f32)>>,
    pub clusters: Vec<Rect>,
    pub width: f32,
    pub height: f32,
}

/// One node of the layered graph: a real node or a dummy on a long edge.
#[derive(Clone, Debug)]
struct L {
    w: f32,
    h: f32,
    rank: usize,
    /// Clusters from the outermost to the innermost.
    path: Vec<usize>,
    dummy: bool,
    /// Holds a cluster's place on a rank it has no node on, so whatever is
    /// on that rank stays outside its box.
    filler: Option<usize>,
    /// Neighbours in the layer above / below, with weights.
    up: Vec<(usize, f32)>,
    down: Vec<(usize, f32)>,
}

pub fn layout(g: &Graph, sp: &Spacing) -> Out {
    let n = g.nodes.len();
    let nc = g.clusters.len();
    let parent = |c: usize| g.clusters.get(c).and_then(|c| c.parent).filter(|&p| p < nc);
    // cluster paths, outermost first (a cycle in parents stops at 64)
    let path_of = |c: Option<usize>| -> Vec<usize> {
        let mut p = Vec::new();
        let mut at = c.filter(|&c| c < nc);
        while let Some(c) = at {
            if p.contains(&c) || p.len() > 64 { break }
            p.push(c);
            at = parent(c);
        }
        p.reverse();
        p
    };
    let valid: Vec<usize> = (0..g.edges.len()).filter(|&i| { let e = &g.edges[i]; e.from < n && e.to < n && e.from != e.to }).collect();

    // 1. acyclic: DFS, back edges reversed
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &i in &valid { out_edges[g.edges[i].from].push(i); }
    let mut reversed = vec![false; g.edges.len()];
    let mut state = vec![0u8; n];
    for root in 0..n {
        if state[root] != 0 { continue }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(top) = stack.last_mut() {
            let v = top.0;
            if let Some(&e) = out_edges[v].get(top.1) {
                top.1 += 1;
                let w = g.edges[e].to;
                match state[w] {
                    0 => { state[w] = 1; stack.push((w, 0)); }
                    1 => reversed[e] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    let ends = |i: usize| { let e = &g.edges[i]; if reversed[i] { (e.to, e.from) } else { (e.from, e.to) } };
    let span = |i: usize| 2 * g.edges[i].minlen.clamp(1, 32);

    // 2. ranks
    let mut ins: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut outs: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &i in &valid { let (u, v) = ends(i); outs[u].push(i); ins[v].push(i); }
    let mut indeg: Vec<usize> = ins.iter().map(Vec::len).collect();
    let mut topo: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut k = 0;
    while k < topo.len() {
        let u = topo[k];
        k += 1;
        for &i in &outs[u] { let v = ends(i).1; indeg[v] -= 1; if indeg[v] == 0 { topo.push(v); } }
    }
    // (the DFS leaves no cycles; should one remain, its nodes go last)
    if topo.len() < n { for v in 0..n { if !topo.contains(&v) { topo.push(v); } } }
    let mut rank = vec![0usize; n];
    for &u in &topo {
        for &i in &outs[u] { let v = ends(i).1; rank[v] = rank[v].max(rank[u] + span(i)); }
    }
    for _ in 0..4 {
        for &v in topo.iter().rev().chain(topo.iter()) {
            let lo = ins[v].iter().map(|&i| rank[ends(i).0] + span(i)).max();
            let hi = outs[v].iter().filter_map(|&i| rank[ends(i).1].checked_sub(span(i))).min();
            let (a, b) = (ins[v].len(), outs[v].len());
            rank[v] = match (lo, hi) {
                (None, Some(h)) => h,
                (Some(l), None) => l,
                (Some(l), Some(h)) if l <= h => if a > b { l } else if b > a { h } else { rank[v].clamp(l, h) },
                _ => rank[v],
            };
        }
    }
    let min = rank.iter().copied().min().unwrap_or(0);
    for r in &mut rank { *r -= min; }

    // 3. the layered graph
    let mut ls: Vec<L> = (0..n).map(|v| L { w: g.nodes[v].w.max(0.), h: g.nodes[v].h.max(0.), rank: rank[v], path: path_of(g.nodes[v].cluster), dummy: false, filler: None, up: Vec::new(), down: Vec::new() }).collect();
    let mut chains: Vec<Vec<usize>> = vec![Vec::new(); g.edges.len()];
    let mut label_at: Vec<Option<usize>> = vec![None; g.edges.len()];
    for &i in &valid {
        let (u, v) = ends(i);
        let (ru, rv) = (rank[u], rank[v]);
        let common = {
            let (a, b) = (&ls[u].path, &ls[v].path);
            a.iter().zip(b).take_while(|(x, y)| x == y).map(|(x, _)| *x).collect::<Vec<_>>()
        };
        let mut chain = vec![u];
        let mid = ru + (rv - ru) / 2;
        for r in ru + 1..rv {
            let (w, h) = if r == mid { g.edges[i].label } else { (0., 0.) };
            if r == mid && (w > 0. || h > 0.) { label_at[i] = Some(ls.len()); }
            chain.push(ls.len());
            ls.push(L { w, h, rank: r, path: common.clone(), dummy: true, filler: None, up: Vec::new(), down: Vec::new() });
        }
        chain.push(v);
        for p in chain.windows(2) {
            let (a, b) = (p[0], p[1]);
            let wt = match (ls[a].dummy, ls[b].dummy) { (true, true) => 8., (false, false) => 1., _ => 2. };
            ls[a].down.push((b, wt));
            ls[b].up.push((a, wt));
        }
        chains[i] = chain;
    }
    let layers_n = ls.iter().map(|l| l.rank + 1).max().unwrap_or(0);
    // fillers on the ranks a cluster spans but has nothing on
    {
        let mut has = vec![vec![false; layers_n]; nc];
        for l in &ls { for &c in &l.path { has[c][l.rank] = true; } }
        for c in 0..nc {
            let (Some(lo), Some(hi)) = (has[c].iter().position(|&b| b), has[c].iter().rposition(|&b| b)) else { continue };
            for r in lo..=hi {
                if !has[c][r] { ls.push(L { w: 0., h: 0., rank: r, path: path_of(Some(c)), dummy: true, filler: Some(c), up: Vec::new(), down: Vec::new() }); }
            }
        }
    }

    // 4. order
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); layers_n];
    {
        // first order: DFS down from the nodes in rank order
        let mut by_rank: Vec<usize> = (0..ls.len()).collect();
        by_rank.sort_by_key(|&v| (ls[v].rank, v));
        let mut seen = vec![false; ls.len()];
        for s in by_rank {
            if seen[s] { continue }
            let mut stack = vec![s];
            while let Some(v) = stack.pop() {
                if seen[v] { continue }
                seen[v] = true;
                layers[ls[v].rank].push(v);
                for &(w, _) in ls[v].down.iter().rev() { if !seen[w] { stack.push(w); } }
            }
        }
    }
    let mut pos = vec![0f32; ls.len()];
    let place = |layers: &Vec<Vec<usize>>, pos: &mut Vec<f32>| {
        for l in layers { let len = l.len().max(1) as f32; for (i, &v) in l.iter().enumerate() { pos[v] = (i as f32 + 0.5) / len; } }
    };
    place(&layers, &mut pos);
    // a layer sorted by `key` per node, clusters together, ordered by `ckey`
    let sort_layer = |layer: &mut Vec<usize>, key: &[f32], ckey: &[f32], ls: &[L]| {
        layer.sort_by(|&a, &b| {
            let (pa, pb) = (&ls[a].path, &ls[b].path);
            let i = pa.iter().zip(pb).take_while(|(x, y)| x == y).count();
            let item = |p: &Vec<usize>, v: usize| match p.get(i) { Some(&c) => (ckey.get(c).copied().unwrap_or(0.), 0, c), None => (key[v], 1, v) };
            let (x, y) = (item(pa, a), item(pb, b));
            x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2))
        });
    };
    // cluster keys: the mean position of their members in `layers`
    let cluster_keys = |members: &mut dyn Iterator<Item = usize>, pos: &[f32], ls: &[L]| -> Vec<f32> {
        let mut sum = vec![(0f32, 0f32); nc];
        for v in members { for &c in &ls[v].path { sum[c].0 += pos[v]; sum[c].1 += 1.; } }
        sum.iter().map(|&(s, k)| if k > 0. { s / k } else { 0. }).collect()
    };
    let normalize = |layers: &mut Vec<Vec<usize>>, pos: &mut Vec<f32>, ls: &[L]| {
        let ck = cluster_keys(&mut (0..ls.len()), pos, ls);
        let key = pos.clone();
        for l in layers.iter_mut() { sort_layer(l, &key, &ck, ls); }
        place(layers, pos);
    };
    let crossings = |layers: &Vec<Vec<usize>>, pos: &[f32], ls: &[L]| -> f32 {
        let mut total = 0.;
        for l in layers {
            let mut es: Vec<(f32, f32, f32)> = Vec::new();
            for &v in l { for &(w, wt) in &ls[v].down { es.push((pos[v], pos[w], wt)); } }
            for i in 0..es.len() { for j in i + 1..es.len() {
                let (a, b) = (es[i], es[j]);
                if (a.0 - b.0) * (a.1 - b.1) < 0. { total += a.2 * b.2; }
            } }
        }
        total
    };
    normalize(&mut layers, &mut pos, &ls);
    let mut best = (crossings(&layers, &pos, &ls), layers.clone());
    let mut stale = 0;
    for it in 0..24 {
        if best.0 == 0. || stale >= 4 { break }
        let down = it % 2 == 0;
        let order: Vec<usize> = if down { (1..layers_n).collect() } else { (0..layers_n.saturating_sub(1)).rev().collect() };
        for r in order {
            let mut key = pos.clone();
            for &v in &layers[r] {
                let nb = if down { &ls[v].up } else { &ls[v].down };
                let (s, k) = nb.iter().fold((0., 0.), |(s, k), &(w, wt)| (s + pos[w] * wt, k + wt));
                if k > 0. { key[v] = s / k; }
            }
            let ck = cluster_keys(&mut layers[r].iter().copied(), &key, &ls);
            let mut l = std::mem::take(&mut layers[r]);
            sort_layer(&mut l, &key, &ck, &ls);
            let len = l.len().max(1) as f32;
            for (i, &v) in l.iter().enumerate() { pos[v] = (i as f32 + 0.5) / len; }
            layers[r] = l;
        }
        normalize(&mut layers, &mut pos, &ls);
        let c = crossings(&layers, &pos, &ls);
        if c < best.0 { best = (c, layers.clone()); stale = 0; } else { stale += 1; }
    }
    let layers = best.1;
    place(&layers, &mut pos);

    // 5. x by constraints
    let depth_extra = |path: &[usize], from: usize, side: usize| -> f32 {
        path[from.min(path.len())..].iter().map(|&c| sp.pad + g.clusters[c].extra[side]).sum()
    };
    let sep = |a: &L, b: &L| {
        let s = |l: &L| if !l.dummy { sp.node } else if l.w > 0. { sp.node / 2. } else { sp.edge };
        (s(a) + s(b)) / 2.
    };
    let mut cons: Vec<(usize, usize, f32)> = Vec::new();
    let mut item_pairs: Vec<((usize, bool), (usize, bool), usize)> = Vec::new();
    for l in &layers {
        for p in l.windows(2) {
            let (a, b) = (p[0], p[1]);
            let (pa, pb) = (&ls[a].path, &ls[b].path);
            let i = pa.iter().zip(pb).take_while(|(x, y)| x == y).count();
            let d = ls[a].w / 2. + ls[b].w / 2. + sep(&ls[a], &ls[b]) + depth_extra(pa, i, 1) + depth_extra(pb, i, 3);
            cons.push((a, b, d));
            let ia = pa.get(i).map_or((a, false), |&c| (c, true));
            let ib = pb.get(i).map_or((b, false), |&c| (c, true));
            if (ia.1 || ib.1) && !item_pairs.contains(&(ia, ib, i)) { item_pairs.push((ia, ib, i)); }
        }
    }
    let members = |it: (usize, bool)| -> Vec<usize> { if it.1 { (0..ls.len()).filter(|&v| ls[v].path.contains(&it.0)).collect() } else { vec![it.0] } };
    let mut cross: Vec<(usize, usize, f32)> = Vec::new();
    for &(ia, ib, i) in &item_pairs {
        let (ma, mb) = (members(ia), members(ib));
        if ma.len() * mb.len() > 40_000 { continue }
        for &a in &ma { for &b in &mb {
            if ls[a].rank == ls[b].rank { continue }
            let d = ls[a].w / 2. + ls[b].w / 2. + sep(&ls[a], &ls[b]) + depth_extra(&ls[a].path, i, 1) + depth_extra(&ls[b].path, i, 3);
            cross.push((a, b, d));
        } }
    }
    let m = ls.len();
    let solve = |cons: &[(usize, usize, f32)]| -> Option<(Vec<usize>, Vec<Vec<(usize, f32)>>, Vec<Vec<(usize, f32)>>)> {
        let mut before: Vec<Vec<(usize, f32)>> = vec![Vec::new(); m];
        let mut after: Vec<Vec<(usize, f32)>> = vec![Vec::new(); m];
        let mut deg = vec![0usize; m];
        for &(a, b, d) in cons { after[a].push((b, d)); before[b].push((a, d)); deg[b] += 1; }
        let mut order: Vec<usize> = (0..m).filter(|&v| deg[v] == 0).collect();
        let mut k = 0;
        while k < order.len() {
            let v = order[k];
            k += 1;
            for &(w, _) in &after[v] { deg[w] -= 1; if deg[w] == 0 { order.push(w); } }
        }
        (order.len() == m).then_some((order, before, after))
    };
    cons.extend(cross.iter().copied());
    let (order, before, after) = match solve(&cons) {
        Some(s) => s,
        None => {
            // inconsistent cluster order (shouldn't happen): layers only
            cons.truncate(cons.len() - cross.len());
            solve(&cons).unwrap_or_else(|| ((0..m).collect(), vec![Vec::new(); m], vec![Vec::new(); m]))
        }
    };
    let mut x = vec![0f32; m];
    for &v in &order { for &(a, d) in &before[v] { x[v] = x[v].max(x[a] + d); } }
    // a filler follows the middle of its cluster's own nodes
    let mut owned: Vec<Vec<usize>> = vec![Vec::new(); nc];
    for (v, l) in ls.iter().enumerate() { if l.filler.is_none() { for &c in &l.path { owned[c].push(v); } } }
    let desired = |v: usize, x: &[f32]| -> Option<f32> {
        if let Some(c) = ls[v].filler {
            let o = &owned[c];
            return (!o.is_empty()).then(|| o.iter().map(|&w| x[w]).sum::<f32>() / o.len() as f32);
        }
        let (s, k) = ls[v].up.iter().chain(&ls[v].down).fold((0., 0.), |(s, k), &(w, wt)| (s + x[w] * wt, k + wt));
        (k > 0.).then(|| s / k)
    };
    // relaxed until nothing moves (a long chain straightens slowly)
    for _ in 0..400 {
        let mut moved = 0f32;
        for &v in order.iter().rev() {
            let hi = after[v].iter().map(|&(b, d)| x[b] - d).fold(f32::INFINITY, f32::min);
            if let Some(t) = desired(v, &x) { let t = t.min(hi); if t > x[v] { moved = moved.max(t - x[v]); x[v] = t; } }
        }
        for &v in &order {
            let lo = before[v].iter().map(|&(a, d)| x[a] + d).fold(f32::NEG_INFINITY, f32::max);
            if let Some(t) = desired(v, &x) { let t = t.max(lo); if t < x[v] { moved = moved.max(x[v] - t); x[v] = t; } }
        }
        if moved < 0.05 { break }
    }

    // y by layers
    let mut lh = vec![0f32; layers_n];
    for l in &ls { lh[l.rank] = lh[l.rank].max(l.h); }
    // cluster rank spans and how far their boxes reach past their members
    let mut span_of = vec![(usize::MAX, 0usize); nc];
    for l in &ls { for &c in &l.path { span_of[c].0 = span_of[c].0.min(l.rank); span_of[c].1 = span_of[c].1.max(l.rank); } }
    let mut reach = vec![(0f32, 0f32); nc];
    let mut by_depth: Vec<usize> = (0..nc).collect();
    by_depth.sort_by_key(|&c| std::cmp::Reverse(path_of(Some(c)).len()));
    for &c in &by_depth {
        let mut r = (sp.pad + g.clusters[c].extra[0], sp.pad + g.clusters[c].extra[2]);
        for d in 0..nc {
            if parent(d) == Some(c) && span_of[d].0 != usize::MAX {
                if span_of[d].0 == span_of[c].0 { r.0 = r.0.max(reach[d].0 + sp.pad + g.clusters[c].extra[0]); }
                if span_of[d].1 == span_of[c].1 { r.1 = r.1.max(reach[d].1 + sp.pad + g.clusters[c].extra[2]); }
            }
        }
        reach[c] = r;
    }
    let mut ly = vec![0f32; layers_n];
    let mut y = 0.;
    for r in 0..layers_n {
        if r > 0 {
            // room for the boxes ending on r-1 and those starting on r
            let bottom = (0..nc).filter(|&c| span_of[c].0 != usize::MAX && span_of[c].1 + 1 == r).map(|c| reach[c].1).fold(0., f32::max);
            let top = (0..nc).filter(|&c| span_of[c].0 == r).map(|c| reach[c].0).fold(0., f32::max);
            y += sp.rank / 2. + top + bottom;
        }
        ly[r] = y + lh[r] / 2.;
        y += lh[r];
    }

    // cluster boxes, innermost first
    let mut boxes = vec![Rect::default(); nc];
    let mut have = vec![false; nc];
    let union = |b: &mut Rect, h: &mut bool, r: Rect| {
        if !*h { *b = r; *h = true; return }
        let (x0, y0) = (b.x.min(r.x), b.y.min(r.y));
        let (x1, y1) = ((b.x + b.w).max(r.x + r.w), (b.y + b.h).max(r.y + r.h));
        *b = Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    };
    for &c in &by_depth {
        let (mut b, mut h) = (Rect::default(), false);
        for (v, l) in ls.iter().enumerate() {
            if l.path.last() == Some(&c) { union(&mut b, &mut h, Rect { x: x[v] - l.w / 2., y: ly[l.rank] - l.h / 2., w: l.w, h: l.h }); }
        }
        for d in 0..nc { if parent(d) == Some(c) && have[d] { union(&mut b, &mut h, boxes[d]); } }
        if !h { continue }
        let e = g.clusters[c].extra;
        let p = sp.pad;
        b = Rect { x: b.x - p - e[3], y: b.y - p - e[0], w: b.w + 2. * p + e[1] + e[3], h: b.h + 2. * p + e[0] + e[2] };
        let (mw, mh) = g.clusters[c].min;
        if b.w < mw { b.x -= (mw - b.w) / 2.; b.w = mw; }
        if b.h < mh { b.y -= (mh - b.h) / 2.; b.h = mh; }
        boxes[c] = b;
        have[c] = true;
    }

    // to the origin
    let mut x0 = f32::INFINITY;
    let mut y0 = f32::INFINITY;
    let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for (v, l) in ls.iter().enumerate() { x0 = x0.min(x[v] - l.w / 2.); x1 = x1.max(x[v] + l.w / 2.); y0 = y0.min(ly[l.rank] - l.h / 2.); y1 = y1.max(ly[l.rank] + l.h / 2.); }
    for c in 0..nc { if have[c] { let b = boxes[c]; x0 = x0.min(b.x); y0 = y0.min(b.y); x1 = x1.max(b.x + b.w); y1 = y1.max(b.y + b.h); } }
    if !x0.is_finite() { return Out { nodes: Vec::new(), edges: vec![Vec::new(); g.edges.len()], labels: vec![None; g.edges.len()], clusters: boxes, width: 0., height: 0. } }
    let at = |v: usize| (x[v] - x0, ly[ls[v].rank] - y0);
    let nodes: Vec<(f32, f32)> = (0..n).map(at).collect();
    let mut edges = vec![Vec::new(); g.edges.len()];
    let mut labels = vec![None; g.edges.len()];
    for (i, e) in g.edges.iter().enumerate() {
        if e.from >= n || e.to >= n { continue }
        if e.from == e.to {
            // a loop out of the right side and back
            let (cx, cy) = nodes[e.from];
            let (w, h) = (g.nodes[e.from].w, g.nodes[e.from].h);
            let r = cx + w / 2.;
            let out = 24. + e.label.0;
            edges[i] = vec![(r, cy - h / 4.), (r + out * 0.6, cy - h / 2. - 6.), (r + out, cy), (r + out * 0.6, cy + h / 2. + 6.), (r, cy + h / 4.)];
            if e.label.0 > 0. { labels[i] = Some((r + out - e.label.0 / 2., cy)); }
            continue;
        }
        let mut pts: Vec<(f32, f32)> = chains[i].iter().map(|&v| at(v)).collect();
        if reversed[i] { pts.reverse(); }
        edges[i] = pts;
        labels[i] = label_at[i].map(at);
    }
    let clusters = boxes.iter().map(|b| Rect { x: b.x - x0, y: b.y - y0, ..*b }).collect();
    Out { nodes, edges, labels, clusters, width: x1 - x0, height: y1 - y0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(c: Option<usize>) -> Node { Node { w: 60., h: 30., cluster: c } }
    fn edge(a: usize, b: usize) -> Edge { Edge { from: a, to: b, minlen: 1, label: (0., 0.) } }
    const SP: Spacing = Spacing { node: 50., rank: 50., edge: 10., pad: 8. };

    fn overlap(a: Rect, b: Rect) -> bool { a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h }
    fn rect(o: &Out, g: &Graph, v: usize) -> Rect { let (x, y) = o.nodes[v]; Rect { x: x - g.nodes[v].w / 2., y: y - g.nodes[v].h / 2., w: g.nodes[v].w, h: g.nodes[v].h } }

    #[test]
    fn chain_goes_down() {
        let g = Graph { nodes: vec![node(None); 3], edges: vec![edge(0, 1), edge(1, 2)], clusters: vec![] };
        let o = layout(&g, &SP);
        assert!(o.nodes[0].1 < o.nodes[1].1 && o.nodes[1].1 < o.nodes[2].1);
        assert!((o.nodes[0].0 - o.nodes[2].0).abs() < 1.);
    }

    #[test]
    fn cycles_and_fans() {
        let g = Graph { nodes: vec![node(None); 5], edges: vec![edge(0, 1), edge(0, 2), edge(0, 3), edge(3, 0), edge(1, 4), edge(2, 4), edge(4, 4)], clusters: vec![] };
        let o = layout(&g, &SP);
        for a in 0..5 { for b in a + 1..5 { assert!(!overlap(rect(&o, &g, a), rect(&o, &g, b)), "{a} {b} {:?}", o.nodes); } }
        // the parent sits over its children
        let mid = (o.nodes[1].0 + o.nodes[2].0 + o.nodes[3].0) / 3.;
        assert!((o.nodes[0].0 - mid).abs() < 40., "{:?}", o.nodes);
        assert_eq!(o.edges[6].len(), 5);
    }

    #[test]
    fn clusters_hold_their_nodes_and_nothing_else() {
        // a -> b in cluster 0, c -> d in cluster 1, e outside linked to both
        let mut nodes = vec![node(Some(0)), node(Some(0)), node(Some(1)), node(Some(1)), node(None), node(Some(2))];
        nodes[5].w = 20.;
        let clusters = vec![Cluster { parent: None, extra: [20., 0., 0., 0.], min: (0., 0.) }, Cluster { parent: None, extra: [20., 0., 0., 0.], min: (0., 0.) }, Cluster { parent: Some(1), extra: [10., 0., 0., 0.], min: (100., 0.) }];
        let g = Graph { nodes, edges: vec![edge(0, 1), edge(2, 3), edge(4, 0), edge(4, 3), edge(1, 2), edge(3, 5), edge(4, 5)], clusters };
        let o = layout(&g, &SP);
        let inside = |r: Rect, b: Rect| r.x >= b.x && r.y >= b.y && r.x + r.w <= b.x + b.w && r.y + r.h <= b.y + b.h;
        for v in 0..6 {
            for c in 0..3 {
                let member = g.nodes[v].cluster == Some(c) || (c == 1 && v == 5);
                assert_eq!(inside(rect(&o, &g, v), o.clusters[c]), member, "node {v} cluster {c}: {:?} {:?}", rect(&o, &g, v), o.clusters[c]);
                if !member { assert!(!overlap(rect(&o, &g, v), o.clusters[c]), "node {v} in cluster {c}"); }
            }
        }
        assert!(!overlap(o.clusters[0], o.clusters[1]), "{:?}", o.clusters);
        assert!(o.clusters[2].w >= 100.);
    }

    #[test]
    fn labels_sit_between() {
        let g = Graph { nodes: vec![node(None); 2], edges: vec![Edge { label: (40., 18.), ..edge(0, 1) }], clusters: vec![] };
        let o = layout(&g, &SP);
        let l = o.labels[0].expect("label");
        assert!(l.1 > o.nodes[0].1 + 15. + 9. && l.1 < o.nodes[1].1 - 15. - 9., "{l:?} {:?}", o.nodes);
    }

    /// Random graphs with nested clusters: no two nodes overlap, every node
    /// is inside its clusters' boxes and outside every other box.
    #[test]
    fn random_graphs_keep_their_invariants() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = |n: usize| { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed % n.max(1) as u64) as usize };
        for round in 0..300 {
            let nc = rnd(5);
            let clusters: Vec<Cluster> = (0..nc).map(|c| Cluster { parent: if c > 0 && rnd(2) == 0 { Some(rnd(c)) } else { None }, extra: [rnd(2) as f32 * 18., 0., 0., 0.], min: (rnd(2) as f32 * 120., 0.) }).collect();
            let n = 2 + rnd(14);
            let nodes: Vec<Node> = (0..n).map(|_| Node { w: 20. + rnd(80) as f32, h: 20. + rnd(30) as f32, cluster: if nc > 0 && rnd(3) > 0 { Some(rnd(nc)) } else { None } }).collect();
            let edges: Vec<Edge> = (0..rnd(2 * n)).map(|_| Edge { from: rnd(n), to: rnd(n), minlen: 1 + rnd(2), label: if rnd(3) == 0 { (30., 16.) } else { (0., 0.) } }).collect();
            let g = Graph { nodes, edges, clusters };
            let o = layout(&g, &SP);
            let within = |v: usize, c: usize| { let mut at = g.nodes[v].cluster; let mut k = 0; while let Some(x) = at { if x == c { return true } at = g.clusters[x].parent; k += 1; if k > 10 { break } } false };
            for a in 0..n {
                for b in a + 1..n { assert!(!overlap(rect(&o, &g, a), rect(&o, &g, b)), "round {round}: nodes {a} {b}"); }
                for c in 0..nc {
                    let (r, bx) = (rect(&o, &g, a), o.clusters[c]);
                    if bx.w == 0. { continue }
                    let inside = r.x >= bx.x - 0.01 && r.y >= bx.y - 0.01 && r.x + r.w <= bx.x + bx.w + 0.01 && r.y + r.h <= bx.y + bx.h + 0.01;
                    if within(a, c) { assert!(inside, "round {round}: node {a} out of cluster {c}"); } else { assert!(!overlap(r, bx), "round {round}: node {a} in cluster {c} {:?} {:?}", r, bx); }
                }
            }
        }
    }

    #[test]
    fn junk_does_not_panic() {
        let g = Graph { nodes: vec![node(Some(7)), node(Some(0))], edges: vec![edge(0, 9), edge(1, 1), edge(1, 0)], clusters: vec![Cluster { parent: Some(0), extra: [0.; 4], min: (0., 0.) }] };
        let _ = layout(&g, &SP);
        let _ = layout(&Graph { nodes: vec![], edges: vec![], clusters: vec![] }, &SP);
    }
}
