/* Mesh3D — tiny procedural mesh library + flat-shaded canvas renderer.
   No dependencies. window.Mesh3D = { build(kind), render(canvas, mesh, opts) } */
(function () {
  'use strict';

  // ---------- mesh core ----------
  function mesh() { return { v: [], f: [] }; }
  function addMesh(a, b) {
    const o = a.v.length;
    for (const p of b.v) a.v.push(p);
    for (const f of b.f) a.f.push([f[0] + o, f[1] + o, f[2] + o]);
    return a;
  }
  function combine(list) { const m = mesh(); for (const x of list) addMesh(m, x); return m; }
  function tx(m, fn) { m.v = m.v.map(fn); return m; }
  function move(m, dx, dy, dz) { return tx(m, p => [p[0] + dx, p[1] + dy, p[2] + dz]); }
  function rotY(m, a) { const c = Math.cos(a), s = Math.sin(a); return tx(m, p => [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c]); }
  function rotX(m, a) { const c = Math.cos(a), s = Math.sin(a); return tx(m, p => [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c]); }
  function rotZ(m, a) { const c = Math.cos(a), s = Math.sin(a); return tx(m, p => [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]); }
  function scale(m, sx, sy, sz) { return tx(m, p => [p[0] * sx, p[1] * sy, p[2] * sz]); }

  // ---------- generators ----------
  // profile: [[radius, y], ...] bottom -> top
  function lathe(profile, seg) {
    seg = seg || 20;
    const m = mesh(), n = profile.length;
    for (let j = 0; j < seg; j++) {
      const a = j / seg * Math.PI * 2, ca = Math.cos(a), sa = Math.sin(a);
      for (let i = 0; i < n; i++) m.v.push([profile[i][0] * ca, profile[i][1], profile[i][0] * sa]);
    }
    for (let j = 0; j < seg; j++) {
      const j2 = (j + 1) % seg;
      for (let i = 0; i < n - 1; i++) {
        const a = j * n + i, b = j2 * n + i, c = j2 * n + i + 1, d = j * n + i + 1;
        m.f.push([a, b, c], [a, c, d]);
      }
    }
    if (profile[0][0] > 0.002) {
      const ci = m.v.length; m.v.push([0, profile[0][1], 0]);
      for (let j = 0; j < seg; j++) m.f.push([ci, ((j + 1) % seg) * n, j * n]);
    }
    if (profile[n - 1][0] > 0.002) {
      const ci = m.v.length; m.v.push([0, profile[n - 1][1], 0]);
      for (let j = 0; j < seg; j++) m.f.push([ci, j * n + n - 1, ((j + 1) % seg) * n + n - 1]);
    }
    return m;
  }

  // poly: [[x,z],...] counter-clockwise, extruded 0..h along y. Caps via centroid fan (star-shaped polys).
  function extrude(poly, h) {
    const m = mesh(), n = poly.length;
    for (const p of poly) m.v.push([p[0], 0, p[1]]);
    for (const p of poly) m.v.push([p[0], h, p[1]]);
    for (let i = 0; i < n; i++) {
      const j = (i + 1) % n;
      m.f.push([i, j, n + j], [i, n + j, n + i]);
    }
    let cx = 0, cz = 0;
    for (const p of poly) { cx += p[0]; cz += p[1]; }
    cx /= n; cz /= n;
    const b = m.v.length; m.v.push([cx, 0, cz]);
    const t = m.v.length; m.v.push([cx, h, cz]);
    for (let i = 0; i < n; i++) {
      const j = (i + 1) % n;
      m.f.push([b, j, i], [t, n + i, n + j]);
    }
    return m;
  }

  function box(cx, cy, cz, w, h, d, ry) {
    let m = extrude([[-w / 2, -d / 2], [w / 2, -d / 2], [w / 2, d / 2], [-w / 2, d / 2]], h);
    if (ry) m = rotY(m, ry);
    return move(m, cx, cy, cz);
  }

  function ngon(r, sides, rot) {
    const pts = [];
    for (let i = 0; i < sides; i++) {
      const a = i / sides * Math.PI * 2 + (rot || 0);
      pts.push([r * Math.cos(a), r * Math.sin(a)]);
    }
    return pts;
  }

  function spike(r, h, sides) {
    const m = mesh();
    for (let i = 0; i < sides; i++) {
      const a = i / sides * Math.PI * 2;
      m.v.push([r * Math.cos(a), 0, r * Math.sin(a)]);
    }
    const apex = m.v.length; m.v.push([0, h, 0]);
    const cb = m.v.length; m.v.push([0, 0, 0]);
    for (let i = 0; i < sides; i++) {
      const j = (i + 1) % sides;
      m.f.push([i, j, apex], [cb, j, i]);
    }
    return m;
  }

  function cylinder(r, h, seg) { return lathe([[r, 0], [r, h]], seg || 16); }

  // ---------- catalog ----------
  const B = {
    egg() {
      const prof = [];
      const N = 13;
      for (let i = 0; i <= N; i++) {
        const t = i / N;
        const r = 0.66 * Math.pow(Math.sin(Math.PI * Math.min(0.999, Math.max(0.001, t))), 0.85) * (1.04 - 0.3 * t);
        prof.push([Math.max(0.001, r), 0.18 + t * 1.45]);
      }
      const eggM = lathe(prof, 9);
      const base = lathe([[0.52, 0], [0.46, 0.1], [0.3, 0.2], [0.001, 0.22]], 9);
      return combine([base, eggM]);
    },
    vase() {
      return lathe([[0.42, 0], [0.56, 0.05], [0.52, 0.22], [0.36, 0.6], [0.4, 0.95],
        [0.58, 1.22], [0.66, 1.36], [0.6, 1.44], [0.5, 1.46]], 26);
    },
    facetVase() {
      return lathe([[0.5, 0], [0.68, 0.12], [0.74, 0.55], [0.52, 1.02], [0.56, 1.14], [0.62, 1.2]], 6);
    },
    pawn() {
      return lathe([[0.5, 0], [0.5, 0.1], [0.32, 0.18], [0.2, 0.3], [0.16, 0.6], [0.27, 0.7],
        [0.16, 0.78], [0.3, 0.96], [0.27, 1.08], [0.001, 1.16]], 12);
    },
    rook() {
      return lathe([[0.55, 0], [0.55, 0.12], [0.4, 0.22], [0.3, 0.32], [0.27, 0.85], [0.42, 0.95],
        [0.44, 1.16], [0.32, 1.16], [0.31, 1.04], [0.001, 1.04]], 14);
    },
    plinth() {
      return rotY(lathe([[0.95, 0], [0.95, 0.14], [0.74, 0.24], [0.65, 0.36], [0.62, 0.8],
        [0.72, 0.92], [0.9, 0.98], [0.9, 1.12], [0.001, 1.12]], 4), Math.PI / 4);
    },
    feet() {
      const f = lathe([[0.5, 0], [0.46, 0.1], [0.3, 0.34], [0.27, 0.36], [0.001, 0.36]], 14);
      return combine([
        move(scale(combine([f]), 1, 1, 1), -0.65, 0, -0.45),
        move(B._footCopy(), 0.65, 0, -0.45),
        move(B._footCopy(), -0.65, 0, 0.45),
        move(B._footCopy(), 0.65, 0, 0.45)
      ]);
    },
    _footCopy() {
      return lathe([[0.5, 0], [0.46, 0.1], [0.3, 0.34], [0.27, 0.36], [0.001, 0.36]], 14);
    },
    base32() {
      const b = lathe([[0.55, 0], [0.55, 0.08], [0.48, 0.18], [0.001, 0.18]], 22);
      return combine([
        move(B._b32(), -0.62, 0, -0.35), move(B._b32(), 0.62, 0, -0.35),
        move(b, 0, 0, 0.45)
      ]);
    },
    _b32() { return lathe([[0.55, 0], [0.55, 0.08], [0.48, 0.18], [0.001, 0.18]], 22); },
    gear() {
      const teeth = 9, ro = 1, ri = 0.76, pts = [];
      const step = Math.PI * 2 / teeth;
      for (let t = 0; t < teeth; t++) {
        const a = t * step;
        pts.push([ri * Math.cos(a), ri * Math.sin(a)]);
        pts.push([ri * Math.cos(a + step * 0.34), ri * Math.sin(a + step * 0.34)]);
        pts.push([ro * Math.cos(a + step * 0.46), ro * Math.sin(a + step * 0.46)]);
        pts.push([ro * Math.cos(a + step * 0.84), ro * Math.sin(a + step * 0.84)]);
      }
      const g = extrude(pts, 0.3);
      const hub = cylinder(0.3, 0.46, 14);
      return combine([g, hub]);
    },
    coaster() {
      const pts = [];
      for (let i = 0; i < 72; i++) {
        const a = i / 72 * Math.PI * 2;
        const r = 1.02 + 0.17 * Math.pow(Math.abs(Math.sin(a * 4.5)), 0.65);
        pts.push([r * Math.cos(a), r * Math.sin(a)]);
      }
      const top = extrude(ngon(0.78, 24, 0), 0.06);
      return combine([extrude(pts, 0.12), move(top, 0, 0.12, 0)]);
    },
    hexTile() {
      const tile = extrude(ngon(1.35, 6, 0), 0.22);
      const r1 = move(rotZ(spike(0.3, 0.85, 5), 0.18), 0.45, 0.2, 0.3);
      const r2 = move(rotZ(spike(0.22, 0.55, 5), -0.3), -0.4, 0.2, -0.25);
      const r3 = move(spike(0.16, 0.4, 5), 0.1, 0.2, -0.55);
      const rock = move(box(-0.55, 0.22, 0.5, 0.4, 0.26, 0.34, 0.5), 0, 0, 0);
      return combine([tile, r1, r2, r3, rock]);
    },
    diceTower() {
      const tower = box(0, 0.5, -0.25, 1.0, 2.1, 1.0, 0);
      const roofM = move(rotY(spike(0.78, 0.55, 4), Math.PI / 4), 0, 2.6, -0.25);
      const tray = combine([
        box(0, 0, 0.85, 1.5, 0.12, 1.1, 0),
        box(-0.72, 0, 0.85, 0.08, 0.42, 1.1, 0),
        box(0.72, 0, 0.85, 0.08, 0.42, 1.1, 0),
        box(0, 0, 1.36, 1.5, 0.42, 0.08, 0)
      ]);
      const baseM = box(0, 0, -0.25, 1.3, 0.5, 1.3, 0);
      return combine([baseM, tower, roofM, tray]);
    },
    barricade() {
      const parts = [];
      for (let i = 0; i < 5; i++) {
        const x = -1.0 + i * 0.5;
        parts.push(rotX(box(0, 0, 0, 0.34, 1.3 + (i % 2) * 0.22, 0.1, 0), -0.18));
        parts[parts.length - 1] = move(parts[parts.length - 1], x, 0, 0);
      }
      parts.push(move(rotZ(box(0, 0, 0, 2.5, 0.16, 0.1, 0), 0.06), 0, 0.7, 0.14));
      parts.push(box(-0.9, 0, 0.25, 0.22, 0.5, 0.7, 0.3));
      parts.push(box(0.95, 0, 0.3, 0.26, 0.34, 0.6, -0.4));
      return combine(parts);
    },
    sandbags() {
      const parts = [];
      const bag = (x, y, z, ry) => box(x, y, z, 0.6, 0.26, 0.4, ry);
      for (let i = 0; i < 4; i++) parts.push(bag(-1.0 + i * 0.66, 0, 0, (i % 2) * 0.12 - 0.06));
      for (let i = 0; i < 4; i++) parts.push(bag(-1.0 + i * 0.66, 0, 0.45, (i % 2) * -0.1 + 0.05));
      for (let i = 0; i < 3; i++) parts.push(bag(-0.68 + i * 0.66, 0.26, 0.22, (i % 2) * 0.14 - 0.07));
      for (let i = 0; i < 2; i++) parts.push(bag(-0.35 + i * 0.66, 0.52, 0.22, (i % 2) * -0.12 + 0.06));
      return combine(parts);
    },
    bracket() {
      const L = extrude([[0, 0], [1.3, 0], [1.3, 0.26], [0.3, 0.26], [0.3, 1.5], [0, 1.5]], 0.5);
      const rib = extrude([[0.3, 0.26], [0.95, 0.26], [0.3, 1.0]], 0.12);
      return rotX(combine([L, move(rib, 0, 0.19, 0)]), Math.PI / 2);
    },
    hook() {
      const p = extrude([[0, 0], [0.24, 0], [0.24, 0.55], [0.78, 0.55], [0.9, 0.65], [0.9, 0.95],
        [0.68, 0.95], [0.68, 0.78], [0.46, 0.78], [0.46, 1.5], [0, 1.5]], 0.5);
      return rotX(p, Math.PI / 2);
    },
    clip() {
      const pts = [];
      const a0 = 0.7, a1 = Math.PI * 2 - 0.7;
      for (let i = 0; i <= 24; i++) { const a = a0 + (a1 - a0) * i / 24; pts.push([0.52 * Math.cos(a), 0.52 * Math.sin(a)]); }
      for (let i = 24; i >= 0; i--) { const a = a0 + (a1 - a0) * i / 24; pts.push([0.32 * Math.cos(a), 0.32 * Math.sin(a)]); }
      return rotX(extrude(pts, 0.6), Math.PI / 2);
    },
    comb() {
      const parts = [box(0, 0, 0, 2.3, 0.2, 0.55, 0)];
      for (let i = 0; i < 9; i++) parts.push(box(-1.0 + i * 0.25, 0.2, 0, 0.1, 0.55, 0.45, 0));
      return combine(parts);
    },
    tray() {
      const parts = [
        box(0, 0, 0, 2.1, 0.1, 1.4, 0),
        box(-1.01, 0, 0, 0.08, 0.5, 1.4, 0),
        box(1.01, 0, 0, 0.08, 0.5, 1.4, 0),
        box(0, 0, -0.66, 2.1, 0.5, 0.08, 0),
        box(0, 0, 0.66, 2.1, 0.5, 0.08, 0),
        box(-0.34, 0, 0, 0.06, 0.42, 1.4, 0),
        box(0.34, 0, 0, 0.06, 0.42, 1.4, 0)
      ];
      return combine(parts);
    },
    crystals() {
      const baseM = lathe([[0.85, 0], [0.78, 0.1], [0.6, 0.18], [0.001, 0.18]], 8);
      const c1 = move(spike(0.3, 1.35, 6), 0, 0.12, 0);
      const c2 = move(rotZ(spike(0.22, 0.85, 6), 0.4), 0.42, 0.12, 0.18);
      const c3 = move(rotZ(spike(0.17, 0.6, 6), -0.45), -0.38, 0.12, 0.22);
      const c4 = move(rotX(spike(0.14, 0.5, 6), 0.4), 0.05, 0.12, -0.4);
      return combine([baseM, c1, c2, c3, c4]);
    }
  };

  function build(kind) {
    const fn = B[kind] || B.egg;
    return fn();
  }

  // ---------- renderer ----------
  function norm(v) { const l = Math.hypot(v[0], v[1], v[2]) || 1; return [v[0] / l, v[1] / l, v[2] / l]; }

  function render(canvas, m, o) {
    o = o || {};
    const ctx = canvas.getContext('2d');
    const W = canvas.width, H = canvas.height;
    ctx.clearRect(0, 0, W, H);
    if (!m || !m.v.length) return;
    const rx = o.rx == null ? -0.42 : o.rx;
    const ry = o.ry == null ? 0.8 : o.ry;
    const zoom = o.zoom == null ? 1 : o.zoom;
    let col = o.color || '#bcc0c8';
    if (typeof col === 'string') {
      const h = col.replace('#', '');
      col = [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
    }
    // bounds
    const mn = [1e9, 1e9, 1e9], mx = [-1e9, -1e9, -1e9];
    for (const p of m.v) for (let k = 0; k < 3; k++) {
      if (p[k] < mn[k]) mn[k] = p[k];
      if (p[k] > mx[k]) mx[k] = p[k];
    }
    const c = [(mn[0] + mx[0]) / 2, (mn[1] + mx[1]) / 2, (mn[2] + mx[2]) / 2];
    const R = Math.max(mx[0] - mn[0], mx[1] - mn[1], mx[2] - mn[2]) / 2 || 1;
    const cy = Math.cos(ry), sy = Math.sin(ry), cx2 = Math.cos(rx), sx2 = Math.sin(rx);
    const s = (Math.min(W, H) * 0.4 * zoom) / R;
    const rot = [], proj = [];
    for (const p of m.v) {
      const x = p[0] - c[0], y = p[1] - c[1], z = p[2] - c[2];
      const x1 = x * cy + z * sy, z1 = -x * sy + z * cy;
      const y1 = y * cx2 - z1 * sx2, z2 = y * sx2 + z1 * cx2;
      rot.push([x1, y1, z2]);
      const f = 3.4 / (3.4 - (z2 / R) * 0.9);
      proj.push([W / 2 + x1 * s * f, H / 2 - y1 * s * f, z2]);
    }
    // floor shadow
    if (o.floor !== false) {
      const fy = mn[1] - c[1];
      const fz = fy * sx2;
      const ff = 3.4 / (3.4 - (fz / R) * 0.9);
      const px = W / 2, py = H / 2 - (fy * cx2) * s * ff;
      ctx.save();
      const rxp = R * s * 0.92, ryp = R * s * 0.92 * Math.abs(sx2) * 0.6 + 4;
      const g = ctx.createRadialGradient(px, py, 0, px, py, rxp);
      g.addColorStop(0, 'rgba(0,0,0,0.5)');
      g.addColorStop(1, 'rgba(0,0,0,0)');
      ctx.translate(px, py);
      ctx.scale(1, ryp / rxp);
      ctx.translate(-px, -py);
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(px, py, rxp, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
    const light = norm([-0.45, 0.85, 0.65]);
    const order = [];
    for (let i = 0; i < m.f.length; i++) {
      const f = m.f[i];
      const z = (rot[f[0]][2] + rot[f[1]][2] + rot[f[2]][2]) / 3;
      order.push([z, i]);
    }
    order.sort((a, b) => a[0] - b[0]);
    for (const [, i] of order) {
      const f = m.f[i];
      const a = rot[f[0]], b = rot[f[1]], d = rot[f[2]];
      const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
      const v = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
      let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
      n = norm(n);
      const dot = Math.abs(n[0] * light[0] + n[1] * light[1] + n[2] * light[2]);
      const sh = 0.26 + 0.74 * dot;
      const fill = 'rgb(' + Math.round(col[0] * sh) + ',' + Math.round(col[1] * sh) + ',' + Math.round(col[2] * sh) + ')';
      ctx.fillStyle = fill;
      ctx.strokeStyle = fill;
      ctx.lineWidth = 0.7;
      ctx.beginPath();
      ctx.moveTo(proj[f[0]][0], proj[f[0]][1]);
      ctx.lineTo(proj[f[1]][0], proj[f[1]][1]);
      ctx.lineTo(proj[f[2]][0], proj[f[2]][1]);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
    }
  }

  window.Mesh3D = { build, render };
})();
