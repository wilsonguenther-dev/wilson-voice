/* Round three drawings for the pill page: recording headphones, Hoot at small
 * scale, the tape machine, the warm-up machine, and the tall side-dock world. */
(function () {
  "use strict";
  var Y = window.Yappy;
  function H(o) { return { ell: function (cx, cy, rx, ry, col) { o.fillStyle = col; for (var y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) { var q = (y + .5 - cy) / ry; if (Math.abs(q) > 1) continue; var hw = rx * Math.sqrt(1 - q * q); o.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1); } }, rct: function (x, y, w, h, col) { o.fillStyle = col; o.fillRect(Math.round(x), Math.round(y), w, h); } }; }
  /* Headphones whose right ear cup IS the recording light. */
  function recPhones(t) { return function (o, cx, cy, rx, ry, h) {
    h.rct(cx - rx + 1, cy - ry - 1, 2 * rx - 2, 2, "#2a2d35"); h.rct(cx - rx - 2, cy - ry + 1, 2, 6, "#2a2d35"); h.rct(cx + rx, cy - ry + 1, 2, 6, "#2a2d35");
    h.rct(cx - rx - 3, cy - 3, 4, 6, "#3b3f4a"); h.rct(cx - rx - 2, cy - 2, 2, 1, "#5a606e");
    var on = (t % 1.6) < 1.1;
    h.rct(cx + rx - 2, cy - 4, 6, 8, "#2a2d35"); h.rct(cx + rx - 1, cy - 3, 4, 6, on ? "#ff3b3b" : "#9a2a24"); h.rct(cx + rx - 1, cy - 3, 1, 1, on ? "#ffd0d0" : "#c86a64");
  }; }
  /* Hoot drawn at 24 px from the detailed 40 px sprite (nearest-neighbour). */
  var hb = document.createElement("canvas"); hb.width = 40; hb.height = 40;
  function hoot(o, x, y, f, size, rec, t) { var c = hb.getContext("2d"); c.clearRect(0, 0, 40, 40); window.FR.D.hoot.draw(c, window.FR.H(c), f || 0); o.imageSmoothingEnabled = false; var s = size || 24; o.drawImage(hb, Math.round(x), Math.round(y), s, s);
    if (rec) { var on = (t % 1.6) < 1.1, k = s / 40; H(o).rct(x + 26 * k, y + 10 * k, Math.max(2, 4 * k), Math.max(3, 5 * k), on ? "#ff3b3b" : "#9a2a24"); } }
  /* Reel-to-reel tape machine, reels turn while recording. */
  function tape(o, x, y, t) { var h = H(o); h.rct(x, y, 22, 14, "#2a2d35"); h.rct(x + 1, y + 1, 20, 12, "#4a5060"); h.rct(x + 1, y + 1, 20, 1, "#6a7284");
    [[6, 6], [16, 6]].forEach(function (r, i) { h.ell(x + r[0], y + r[1], 4, 4, "#1e2025"); h.ell(x + r[0], y + r[1], 3, 3, "#8b6a3a"); var a = t * 4 + i; h.rct(x + r[0] + Math.round(Math.cos(a) * 2), y + r[1] + Math.round(Math.sin(a) * 2), 1, 1, "#1e2025"); h.rct(x + r[0], y + r[1], 1, 1, "#e8e6e1"); });
    h.rct(x + 6, y + 10, 10, 1, "#1e2025"); h.rct(x + 3, y + 12, 3, 1, (t % 1.6) < 1.1 ? "#ff3b3b" : "#9a2a24"); h.rct(x + 16, y + 12, 3, 1, "#7fdc7a"); h.rct(x + 3, y + 14, 2, 3, "#2a2d35"); h.rct(x + 17, y + 14, 2, 3, "#2a2d35"); }
  /* The warm-up machine: a crate engine with a crank, a gauge and a chimney. */
  function engine(o, x, y, pct, t, stage) { var h = H(o); var by = y;
    h.rct(x, by, 18, 14, "#5a3420"); h.rct(x + 1, by + 1, 16, 12, "#b8763f"); h.rct(x + 1, by + 1, 16, 1, "#d8955a"); h.rct(x + 1, by + 7, 16, 1, "#9a5c30");
    h.ell(x + 9, by + 5, 4, 3, "#2a2d35"); h.ell(x + 9, by + 5, 3, 2, "#fffdf5"); var a = -Math.PI + pct / 100 * Math.PI; for (var i = 1; i <= 3; i++) h.rct(x + 9 + Math.round(Math.cos(a) * i), by + 5 + Math.round(Math.sin(a) * i * .7), 1, 1, pct >= 95 ? "#3fa36b" : "#b93a26");
    h.rct(x + 13, by - 5, 3, 5, "#2a2d35"); if (stage >= 2) { var p = Math.floor(t * 3) % 3; h.ell(x + 15 + p, by - 7 - p * 2, 2 + p * .5, 1.5 + p * .5, "rgba(255,255,255,.85)"); }
    if (stage === 1) { var c = t * 8; h.rct(x + 18, by + 8, 3, 2, "#2a2d35"); h.rct(x + 20, by + 9 + Math.round(Math.sin(c) * 3), 1, 1, "#2a2d35"); h.rct(x + 19 + Math.round(Math.cos(c) * 3), by + 9 + Math.round(Math.sin(c) * 3), 3, 2, "#e8c86a"); }
    if (stage === 3) { h.rct(x + 3, by + 9, 12, 3, "#1e2025"); var n = Math.floor(t * 6) % 4; for (var k = 0; k < 2; k++) h.rct(x + 4, by + 9 + k, 2 + ((n + k * 2) % 5) * 2, 1, "#7fdc7a"); }
    if (stage === 4) { h.rct(x + 7, by - 4, 4, 3, "#f2c14e"); h.rct(x + 8, by - 5, 2, 1, "#f2c14e"); h.rct(x + 8, by - 1, 2, 1, "#c98a14"); }
  }
  /* Laptop for the "typing code" beat. */
  function laptop(o, x, y, t) { var h = H(o); h.rct(x, y, 14, 9, "#2a2d35"); h.rct(x + 1, y + 1, 12, 7, "#1e2025"); for (var i = 0; i < 3; i++) h.rct(x + 2 + (i % 2) * 2, y + 2 + i * 2, 3 + ((Math.floor(t * 5) + i) % 4) * 2, 1, i === 1 ? "#ffd23f" : "#7fdc7a"); h.rct(x - 2, y + 9, 18, 2, "#4a5060"); h.rct(x - 2, y + 9, 18, 1, "#6a7284"); }

  /* The tall world used on a side dock: 40 x 128 scene pixels. */
  var TW = 40, TH = 128, tb = document.createElement("canvas"); tb.width = TW; tb.height = TH;
  function tall(A, t) { var o = tb.getContext("2d"), h = H(o); o.clearRect(0, 0, TW, TH);
    h.rct(0, 0, TW, TH, "#eccf92"); for (var i = 0; i < 8; i++) h.rct(0, i, TW, 1, "rgba(150,100,50," + (.2 * (1 - i / 8)) + ")");
    // window with real sky
    h.rct(5, 6, 30, 26, "#5f3c22"); h.rct(7, 8, 26, 22, A.night ? "#171a3d" : "#8fd6ff"); h.rct(7, 8, 26, 6, A.night ? "#2c3060" : "#c6ecff"); h.rct(19, 8, 2, 22, "#5f3c22"); h.rct(7, 18, 26, 2, "#5f3c22");
    var sy = A.sunY != null ? A.sunY : 12; if (A.night) h.ell(28, 12, 2, 2, "#f6f0cf"); else { h.ell(12, sy, 3, 3, "#e8a41f"); h.ell(12, sy, 2, 2, "#ffd23f"); }
    if (A.timeLabel) {} 
    h.rct(3, 32, 34, 2, "#a5704a");
    // top shelf: notepad + word jar
    h.rct(3, 54, 34, 3, "#8a5a36"); h.rct(3, 54, 34, 1, "#b8763f");
    h.rct(6, 44, 9, 10, "#8a7a5a"); h.rct(7, 45, 7, 8, "#fffdf5"); for (var k = 0; k < 3; k++) h.rct(8, 47 + k * 2, 5, 1, "#4a4f5c");
    h.rct(18, 46, 7, 8, "rgba(159,200,208,.7)"); h.rct(18, 45, 7, 1, "#8a5a36"); h.rct(19, 49, 3, 2, "#fffdf5"); h.rct(21, 51, 3, 2, "#fff2b0");
    // middle shelf: typewriter
    h.rct(3, 84, 34, 3, "#8a5a36"); h.rct(3, 84, 34, 1, "#b8763f");
    h.rct(6, 77, 16, 7, "#2a2d35"); h.rct(7, 78, 14, 5, "#4a5060"); h.rct(9, 74, 10, 4, "#fffdf5"); h.rct(10, 75, 7, 1, "#4a4f5c"); for (var q = 0; q < 4; q++) h.rct(8 + q * 3, 81, 2, 1, "#e8e6e1");
    // floor + plant
    h.rct(0, 114, TW, 14, "#a5673f"); h.rct(0, 114, TW, 1, "#bd8151"); h.rct(4, 106, 6, 8, "#c8443a"); h.rct(4, 106, 6, 1, "#e0574a"); h.ell(7, 102, 4, 4, "#5f9f4e"); h.ell(6, 101, 2, 2, "#94dc7c");
    // ladder on the open side
    var lx = A.side === "right" ? 4 : 28; if (A.side === "right") lx = 26;
    h.rct(lx, 36, 1, 78, "#6b4428"); h.rct(lx + 8, 36, 1, 78, "#6b4428"); for (var r = 40; r < 114; r += 7) h.rct(lx, r, 9, 1, "#8a5a36");
    // Yappy: feet at the floor, a shelf, or on the ladder
    var feetY = A.feetY != null ? A.feetY : 114, cx = A.cx != null ? A.cx : 24;
    var a = Y.base(A); Y.drawChick(o, cx, feetY, a);
    if (A.after) A.after(o, cx, feetY, h);
    return tb; }
  function tallWindow(cv, A, t) { var dpr = Math.min(window.devicePixelRatio || 1, 2);
    if (!cv.dataset.w) { cv.dataset.w = cv.getAttribute("width"); cv.dataset.h = cv.getAttribute("height"); }
    var W = +cv.dataset.w, Hh = +cv.dataset.h;
    if (!cv.dataset.css) { cv.style.width = W + "px"; cv.style.height = Hh + "px"; cv.width = W * dpr; cv.height = Hh * dpr; cv.dataset.css = 1; }
    var ctx = cv.getContext("2d"); ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.imageSmoothingEnabled = false; ctx.clearRect(0, 0, W, Hh);
    var b = tall(A, t); var viewH = TW * Hh / W, sy = Math.max(0, Math.min(TH - viewH, (A.focusY != null ? A.focusY : TH) - viewH));
    ctx.save(); ctx.beginPath(); var r = +(cv.dataset.r || 10); ctx.moveTo(r, 0); ctx.arcTo(W, 0, W, Hh, r); ctx.arcTo(W, Hh, 0, Hh, r); ctx.arcTo(0, Hh, 0, 0, r); ctx.arcTo(0, 0, W, 0, r); ctx.closePath(); ctx.clip();
    if (A.side === "right") { ctx.translate(W, 0); ctx.scale(-1, 1); } ctx.drawImage(b, 0, sy, TW, viewH, 0, 0, W, Hh); ctx.restore(); }
  window.R3 = { recPhones: recPhones, hoot: hoot, tape: tape, engine: engine, laptop: laptop, tallWindow: tallWindow, H: H };
})();
