/* Yappy sprite engine for the 2026-11-06 design review.
 * drawChick + drawScene are ported line-for-line from
 * wilson-voice desktop/src/pill/YappyPill.tsx (shipped 0.9.3), so the chick on
 * these pages is the chick in the app. Companion sprites at the bottom are NEW
 * (proposed) and drawn with the same pixel helpers and palette rules. */
(function () {
  "use strict";
  var C = {
    sky1: "#8fd6ff", sky2: "#c6ecff", sun: "#fff2b0", cloud: "#fffaf0", grass: "#8fd06a", grassD: "#6fb84e",
    body: "#ffd24a", light: "#ffe9a0", dark: "#eba21f", outline: "#6b3a2e", cavity: "#7a2e14", belly: "#fff2c4",
    beak: "#ff9f1c", beakD: "#e07a12", eye: "#3a2f36", white: "#ffffff", blush: "#ff89a8", foot: "#f2861a", tongue: "#ff5d7a", sprout: "#e8a41f",
    paper: "#fffdf5", paperL: "#c9c3ad", desk: "#b06a3a", deskD: "#8a4f28", glass: "#2c2a36",
    pencil: "#ffcf3a", pencilT: "#ff8fb0", tw: "#4a4a55", twD: "#33333d", twKey: "#d8d8e2"
  };
  var PW = 176, PH = 44, GY = 30, FX = 88, FY = 18;
  function helpers(o) {
    return {
      ell: function (cx, cy, rx, ry, col) { o.fillStyle = col; for (var y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) { var t = (y + .5 - cy) / ry; if (Math.abs(t) > 1) continue; var hw = rx * Math.sqrt(1 - t * t); o.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1); } },
      rct: function (x, y, w, h, col) { o.fillStyle = col; o.fillRect(Math.round(x), Math.round(y), w, h); }
    };
  }
  function base(A) {
    var d = { hop: 0, sx: 0, sy: 0, blink: 0, mood: 0, beakF: 0, sway: 0, flap: 0, world: 0, prop: "none", propIn: 0, look: "fwd", glasses: false, pencil: false, extra: null };
    for (var k in A) d[k] = A[k]; return d;
  }
  function drawChick(o, cx, feetY, A) {
    var h = helpers(o), ell = h.ell, rct = h.rct;
    var cy = feetY - 11 - A.hop, rx = 11 + A.sx, ry = 11 + A.sy;
    ell(cx + rx - 1, cy + 2, 4, 3, C.outline); ell(cx + rx - 1, cy + 2, 3, 2, C.dark);
    ell(cx + rx - 2, cy + 1 - A.flap, 3, 4, C.outline); ell(cx + rx - 2, cy + 1 - A.flap, 2, 3, C.dark);
    ell(cx, cy, rx + 1, ry + 1, C.outline); ell(cx, cy, rx, ry, C.body); ell(cx - 3, cy - 4, rx - 4, ry - 5, C.light); ell(cx, cy + 5, rx - 5, ry - 5, C.belly);
    var sw = Math.round(A.sway); rct(cx - 1 + sw, cy - ry - 2, 1, 3, C.sprout); rct(cx + 1 + sw, cy - ry - 3, 1, 3, C.sprout);
    var wy = cy + 1 - A.flap;
    ell(cx - rx + 2, wy, 3, 4, C.outline); ell(cx - rx + 2, wy, 2, 3, C.body);
    if (A.pencil) { var px = cx - rx - 1, py = cy + 3; rct(px - 3, py - 3, 1, 1, C.pencilT); rct(px - 2, py - 2, 1, 1, C.pencil); rct(px - 1, py - 1, 1, 1, C.pencil); rct(px, py, 1, 1, C.pencil); rct(px + 1, py + 1, 1, 1, C.dark); }
    var ex = 5, ey = cy + 1;
    [-1, 1].forEach(function (s) {
      var eXc = cx + s * ex;
      if (A.mood > .55) { rct(eXc - 2, ey - 1, 1, 1, C.eye); rct(eXc - 1, ey - 2, 1, 1, C.eye); rct(eXc, ey - 2, 1, 1, C.eye); rct(eXc + 1, ey - 1, 1, 1, C.eye); }
      else if (A.blink > .5 || A.mood < -.5) { rct(eXc - 2, ey + (A.mood < -.5 ? 1 : 0), 4, 1, C.eye); }
      else if (A.look === "down") { ell(eXc, ey + 1, 2.1, 2.3, C.eye); rct(eXc - 1, ey, 1, 1, C.white); }
      else { ell(eXc, ey, 2.2, 2.7, C.eye); rct(eXc - 1, ey - 2, 1, 1, C.white); o.globalAlpha = .7; rct(eXc + 1, ey + 1, 1, 1, C.white); o.globalAlpha = 1; }
    });
    if (A.glasses) {
      rct(cx - ex - 3, ey - 1, 1, 4, C.glass); rct(cx - ex + 2, ey - 1, 1, 4, C.glass); rct(cx - ex - 3, ey - 1, 5, 1, C.glass); rct(cx - ex - 3, ey + 2, 5, 1, C.glass);
      rct(cx + ex - 2, ey - 1, 1, 4, C.glass); rct(cx + ex + 3, ey - 1, 1, 4, C.glass); rct(cx + ex - 2, ey - 1, 5, 1, C.glass); rct(cx + ex - 2, ey + 2, 5, 1, C.glass); rct(cx - 2, ey, 4, 1, C.glass);
    }
    var by = cy + 5, f = A.beakF;
    rct(cx - 1, by - 1, 3, 1, C.beakD); rct(cx - 1, by, 3, 1, C.beak);
    if (f === 1) { rct(cx, by + 1, 1, 1, C.cavity); rct(cx - 1, by + 2, 3, 1, C.beakD); }
    else if (f >= 2) { rct(cx - 1, by + 1, 3, 1, C.cavity); rct(cx, by + 2, 1, 1, C.tongue); rct(cx - 1, by + 3, 3, 1, C.beakD); }
    else { rct(cx, by + 1, 1, 1, C.beak); }
    o.globalAlpha = .7; rct(cx - rx + 2, cy + 3, 2, 1, C.blush); rct(cx + rx - 3, cy + 3, 2, 1, C.blush); o.globalAlpha = 1;
    rct(cx - 3, cy + ry - 1, 1, 2, C.foot); rct(cx + 3, cy + ry - 1, 1, 2, C.foot);
    if (A.extra) A.extra(o, cx, cy, rx, ry, h);
  }
  function drawScene(o, A, elapsed) {
    var h = helpers(o), ell = h.ell, rct = h.rct;
    o.clearRect(0, 0, PW, PH);
    o.globalAlpha = Math.max(0.8, A.world);
    o.fillStyle = A.skyTop || C.sky1; o.fillRect(0, 0, PW, GY);
    o.fillStyle = A.skyHi || C.sky2; o.fillRect(0, 0, PW, 7);
    var sX = A.sunX != null ? A.sunX : 20, sY = A.sunY != null ? A.sunY : 8; if (!A.noSun) { if (!A.night) { if (A.sunCol) ell(sX, sY, 6, 6, "#e8a41f"); ell(sX, sY, 5, 5, A.sunCol || C.sun); } else { ell(sX, sY, 4, 4, "#f4f1d8"); } }
    var cd = (elapsed * 2) % (PW + 20), cX = function (x) { return ((x + cd) % (PW + 20)) - 10; };
    if (!A.night) { ell(cX(120), 7, 7, 2, C.cloud); ell(cX(126), 7, 4, 2, C.cloud); ell(cX(64), 11, 6, 2, C.cloud); }
    else { [[40, 5], [70, 9], [104, 4], [140, 8], [160, 3]].forEach(function (p) { rct(p[0], p[1], 1, 1, "#fffbe0"); }); }
    o.fillStyle = A.night ? "#4f7f45" : C.grass; o.fillRect(0, GY, PW, PH - GY); o.fillStyle = A.night ? "#3e6a37" : C.grassD; o.fillRect(0, GY, PW, 2);
    for (var x = 3; x < PW; x += 9) rct(x, GY - 2, 1, 2, A.night ? "#3e6a37" : C.grassD);
    o.globalAlpha = 1;
    var cx = FX - (A.prop === "desk" ? 6 : 0) + (A.cxOff || 0), feet = GY;
    if (A.hd && A.prop === "pad" && A.propIn > .05) {
      var qx = cx + 12 + (1 - A.propIn) * 18, qy = GY - 14;
      o.globalAlpha = A.propIn;
      rct(qx + 1, qy + 15, 12, 1, "rgba(58,36,24,.25)");
      rct(qx, qy + 1, 12, 14, "#8a7a5a"); rct(qx + 1, qy + 2, 10, 12, C.paper); rct(qx + 1, qy + 13, 10, 1, "#eee6cf"); rct(qx + 10, qy + 2, 1, 12, "#eee6cf");
      rct(qx + 2, qy, 8, 3, "#9aa3ad"); rct(qx + 3, qy, 6, 1, "#c8d0d8"); rct(qx + 5, qy - 1, 2, 1, "#9aa3ad");
      rct(qx + 2, qy + 4, 1, 9, "#f2b8b0");
      for (var j = 0; j < 4; j++) { rct(qx + 3, qy + 5 + j * 2, 7, 1, "#c9d6e6"); rct(qx + 3, qy + 5 + j * 2, j === 3 ? 3 : 5 + (j % 2), 1, "#4a4f5c"); }
      rct(qx + 7, qy + 11, 1, 1, "#4a4f5c");
      o.globalAlpha = 1;
    } else if (A.prop === "pad" && A.propIn > .05) {
      var px = cx + 13 + (1 - A.propIn) * 18, py = GY - 12;
      o.globalAlpha = A.propIn; rct(px, py, 11, 12, C.paperL); rct(px + 1, py + 1, 9, 10, C.paper);
      for (var i = 0; i < 3; i++) rct(px + 2, py + 3 + i * 2, 7, 1, C.paperL); o.globalAlpha = 1;
    }
    if (A.before) A.before(o, cx, feet, h);
    if (!A.hideChick) drawChick(o, cx, feet, A);
    if (A.hd && A.prop === "desk" && A.propIn > .05) {
      var ey = GY - 2 + (1 - A.propIn) * 16;
      o.globalAlpha = Math.min(1, A.propIn + .2);
      rct(4, ey, PW - 8, 9, "#5a3420"); rct(5, ey, PW - 10, 8, "#b8763f"); rct(5, ey, PW - 10, 1, "#d8955a"); rct(5, ey + 2, PW - 10, 1, "#9a5c30");
      for (var g = 12; g < PW - 10; g += 23) { rct(g, ey + 4, 9, 1, "#a8683a"); rct(g + 5, ey + 6, 7, 1, "#a8683a"); }
      rct(5, ey + 7, PW - 10, 1, "#7a4626");
      var ux = cx + 13, uy = ey - 9;
      rct(ux - 1, uy + 9, 22, 1, "rgba(40,20,10,.35)");
      rct(ux, uy + 3, 20, 6, "#2a2d35"); rct(ux + 1, uy + 3, 18, 5, "#4a5060"); rct(ux + 1, uy + 3, 18, 1, "#6a7284");
      rct(ux + 2, uy, 16, 4, "#2a2d35"); rct(ux + 3, uy + 1, 14, 2, "#4a5060"); rct(ux - 1, uy + 1, 3, 2, "#2a2d35"); rct(ux + 18, uy + 1, 3, 2, "#2a2d35"); rct(ux - 1, uy + 1, 1, 1, "#c8d0d8"); rct(ux + 20, uy + 1, 1, 1, "#c8d0d8");
      var lift = Math.floor((elapsed || 0) * 6) % 3 === 0 ? 1 : 0;
      rct(ux + 4, uy - 6 - lift, 12, 7, "#c9c3ad"); rct(ux + 5, uy - 6 - lift, 10, 6, C.paper); rct(ux + 6, uy - 4 - lift, 7, 1, "#4a4f5c"); rct(ux + 6, uy - 2 - lift, 5, 1, "#4a4f5c");
      for (var q = 0; q < 6; q++) { rct(ux + 2 + q * 3, uy + 5, 2, 2, "#1e2025"); rct(ux + 2 + q * 3, uy + 5, 2, 1, q === Math.floor((elapsed || 0) * 9) % 6 ? "#ffd23f" : "#e8e6e1"); }
      rct(ux + 3, uy + 8, 14, 1, "#1e2025");
      var mx = cx - 30, my = ey - 6;
      rct(mx, my, 6, 6, "#5a3420"); rct(mx + 1, my, 4, 5, "#e8e6e1"); rct(mx + 1, my + 1, 4, 1, "#7a4626"); rct(mx + 6, my + 1, 2, 3, "#5a3420"); rct(mx + 2, my - 2, 1, 1, "rgba(255,255,255,.7)"); rct(mx + 3, my - 3, 1, 1, "rgba(255,255,255,.5)");
      rct(mx + 9, my + 1, 1, 5, "#ffcf3a"); rct(mx + 9, my, 1, 1, "#ff8fb0"); rct(mx + 11, my + 2, 1, 4, "#5a9fd6");
      o.globalAlpha = 1;
    } else if (A.prop === "desk" && A.propIn > .05) {
      var dy = GY - 2 + (1 - A.propIn) * 16;
      o.globalAlpha = Math.min(1, A.propIn + .2);
      rct(4, dy, PW - 8, 8, C.desk); rct(4, dy, PW - 8, 2, C.deskD); rct(4, dy + 7, PW - 8, 1, C.deskD);
      var tx = cx + 14, ty = dy - 6;
      rct(tx, ty + 2, 14, 5, C.tw); rct(tx, ty + 2, 14, 1, C.twD); rct(tx + 2, ty, 10, 3, C.tw);
      rct(tx + 4, ty - 3, 6, 3, C.paper);
      for (var k = 0; k < 4; k++) rct(tx + 2 + k * 3, ty + 5, 2, 1, C.twKey);
      rct(cx - 26, dy + 1, 4, 5, C.sun);
      o.globalAlpha = 1;
    }
    if (A.after) A.after(o, cx, feet, h);
  }
  var lerp = function (a, b, t) { return a + (b - a) * t; };
  /* Blit the scene into a window with the app's camera: open=0 is the face
   * close-up the resting pill shows, open=1 is ~80% of the world. */
  function blitWorld(ctx, buf, x0, y0, iw, ih, open) {
    var viewW = lerp(26, PW * 0.8, open), viewH = viewW * (ih / iw);
    var vcx = FX, vcy = lerp(FY, PH * 0.5, open);
    var sx = Math.max(0, Math.min(PW - viewW, vcx - viewW / 2)), sy = Math.max(0, Math.min(PH - viewH, vcy - viewH / 2));
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(buf, sx, sy, viewW, viewH, x0, y0, iw, ih);
  }
  function rr(ctx, x, y, w, h, r) { ctx.beginPath(); ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r); ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath(); }
  function setup(cv) {
    var dpr = Math.min(window.devicePixelRatio || 1, 2), W = +cv.getAttribute("width"), H = +cv.getAttribute("height");
    if (!cv.dataset.css) { cv.style.width = W + "px"; cv.style.height = H + "px"; cv.dataset.css = "1"; cv.width = W * dpr; cv.height = H * dpr; cv.dataset.w = W; cv.dataset.h = H; }
    var ctx = cv.getContext("2d"); ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.imageSmoothingEnabled = false;
    return { ctx: ctx, W: +cv.dataset.w, H: +cv.dataset.h };
  }
  var buf = document.createElement("canvas"); buf.width = PW; buf.height = PH; var bctx = buf.getContext("2d");
  /* The CURRENT shipped capsule (sky gradient, size lerp 72x22 -> 236x60). */
  function currentPill(cv, A, t) {
    var s = setup(cv), ctx = s.ctx; A = base(A);
    var open = A.world, tier = A.prop === "desk" ? 1.16 : A.prop === "pad" ? 1.05 : 1;
    var capW = lerp(72, 236 * tier, open), capH = lerp(22, 60 * tier, open);
    var cx = s.W / 2, cy = s.H / 2, x0 = cx - capW / 2, y0 = cy - capH / 2;
    drawScene(bctx, A, t || 0);
    ctx.clearRect(0, 0, s.W, s.H);
    var g = ctx.createLinearGradient(0, y0, 0, y0 + capH); g.addColorStop(0, "#c6ecff"); g.addColorStop(1, "#8fd6ff");
    ctx.fillStyle = g; rr(ctx, x0, y0, capW, capH, capH / 2); ctx.fill();
    ctx.save(); rr(ctx, x0 + 1, y0 + 1, capW - 2, capH - 2, (capH - 2) / 2); ctx.clip();
    blitWorld(ctx, buf, x0 + 1, y0 + 1, capW - 2, capH - 2, open); ctx.restore();
    ctx.lineWidth = 1; ctx.strokeStyle = "rgba(0,0,0,.18)"; rr(ctx, x0 + .5, y0 + .5, capW - 1, capH - 1, capH / 2); ctx.stroke();
  }
  /* A world WINDOW for the proposed shell: a rounded LCD inside the pill. */
  function worldWindow(cv, A, t) {
    var s = setup(cv), ctx = s.ctx; A = base(A);
    drawScene(bctx, A, t || 0);
    ctx.clearRect(0, 0, s.W, s.H);
    ctx.save(); rr(ctx, 0, 0, s.W, s.H, +(cv.dataset.r || Math.min(s.H / 2, 10))); ctx.clip();
    blitWorld(ctx, buf, 0, 0, s.W, s.H, A.world); ctx.restore();
  }
  /* A bare sprite (chick only, transparent) at integer scale. */
  function sprite(cv, A, scale) {
    var s = setup(cv), ctx = s.ctx; A = base(A);
    var sb = document.createElement("canvas"); sb.width = 40; sb.height = 34; var so = sb.getContext("2d");
    if (A.before) A.before(so, 20, 30, helpers(so));
    drawChick(so, 20, 30, A);
    if (A.after) A.after(so, 20, 30, helpers(so));
    ctx.clearRect(0, 0, s.W, s.H); ctx.imageSmoothingEnabled = false;
    var sc = scale || Math.floor(Math.min(s.W / 40, s.H / 34));
    ctx.drawImage(sb, Math.round((s.W - 40 * sc) / 2), Math.round((s.H - 34 * sc) / 2), 40 * sc, 34 * sc);
  }
  /* Small pixel buffers for proposed companions. */
  function critter(cv, draw, bw, bh) {
    var s = setup(cv), ctx = s.ctx; var sb = document.createElement("canvas"); sb.width = bw; sb.height = bh; var so = sb.getContext("2d");
    draw(so, helpers(so));
    ctx.clearRect(0, 0, s.W, s.H); ctx.imageSmoothingEnabled = false;
    var sc = Math.floor(Math.min(s.W / bw, s.H / bh));
    ctx.drawImage(sb, Math.round((s.W - bw * sc) / 2), Math.round((s.H - bh * sc) / 2), bw * sc, bh * sc);
  }
  var OL = "#4a3328";
  var CRITTERS = {
    hoot: function (o, h, f) { f = f || 0; var e = h.ell, r = h.rct; // owl with headphones
      e(16, 18, 11, 12, OL); e(16, 18, 10, 11, "#a8774f"); e(16, 22, 6, 6, "#e8cfa4");
      r(7, 6, 3, 4, OL); r(22, 6, 3, 4, OL); r(8, 7, 2, 3, "#a8774f"); r(22, 7, 2, 3, "#a8774f");
      e(11, 15, 4, 4, "#fff8e8"); e(21, 15, 4, 4, "#fff8e8");
      if (f === 1) { r(8, 15, 6, 1, "#2b2118"); r(18, 15, 6, 1, "#2b2118"); } else { e(11, 15, 1.8, 2, "#2b2118"); e(21, 15, 1.8, 2, "#2b2118"); r(10, 14, 1, 1, "#fff"); r(20, 14, 1, 1, "#fff"); }
      r(15, 18, 2, 2, "#ff9f1c"); r(15, 20, 2, 1, "#e07a12");
      r(6, 4, 20, 2, "#3b3f4a"); r(4, 6, 2, 8, "#3b3f4a"); r(26, 6, 2, 8, "#3b3f4a"); r(3, 12, 4, 6, "#e0574a"); r(25, 12, 4, 6, "#e0574a");
      r(11, 29, 2, 2, "#f2861a"); r(19, 29, 2, 2, "#f2861a"); },
    shelly: function (o, h, f) { f = f || 0; var e = h.ell, r = h.rct; // snail
      r(3, 22, 26, 5, OL); r(4, 22, 24, 4, "#cfe3b0"); e(25, 18, 4, 5, OL); e(25, 18, 3, 4, "#cfe3b0");
      r(24, 10 - f, 1, 5 + f, OL); r(27, 10 - f, 1, 5 + f, OL); r(23, 9 - f, 3, 2, "#2b2118"); r(26, 9 - f, 3, 2, "#2b2118");
      e(13, 15, 9, 9, OL); e(13, 15, 8, 8, "#7fbf6a"); e(13, 15, 5, 5, "#5f9f4e"); e(13, 15, 2.5, 2.5, "#7fbf6a"); r(13, 15, 1, 1, "#3f7f3a");
      r(9, 9, 4, 2, "#a7dc90");
      r(18, 4, 4, 7, "#3b3f4a"); r(19, 5, 2, 5, "#7fdc7a"); r(19, 3, 2, 1, "#3b3f4a"); },
    tidy: function (o, h, f) { f = f || 0; var e = h.ell, r = h.rct; // ladybug with a broom
      e(16, 18, 10, 9, OL); e(16, 18, 9, 8, "#e0453a"); r(16, 10, 1, 16, OL);
      e(11, 16, 2, 2, "#2b2118"); e(21, 16, 2, 2, "#2b2118"); e(12, 22, 1.6, 1.6, "#2b2118"); e(20, 22, 1.6, 1.6, "#2b2118");
      e(16, 9, 5, 4, "#2b2118"); r(13, 8, 2, 2, "#fff"); r(18, 8, 2, 2, "#fff"); r(14, 9, 1, 1, "#2b2118"); r(19, 9, 1, 1, "#2b2118");
      r(12, 3, 1, 3, "#2b2118"); r(20, 3, 1, 3, "#2b2118");
      r(26 - f, 6, 1, 18, "#a5704a"); r(23 - f, 23, 7, 4, "#e8c86a"); r(23 - f, 26, 7, 1, "#c9a45c"); },
    gulp: function (o, h, f) { f = f || 0; var e = h.ell, r = h.rct; // frog catching a word
      e(15, 20, 12, 9, OL); e(15, 20, 11, 8, "#6cc35a"); e(15, 23, 7, 5, "#c9eba4");
      e(9, 11, 4, 4, OL); e(21, 11, 4, 4, OL); e(9, 11, 3, 3, "#6cc35a"); e(21, 11, 3, 3, "#6cc35a");
      e(9, 11, 1.6, 1.8, "#2b2118"); e(21, 11, 1.6, 1.8, "#2b2118"); r(8, 10, 1, 1, "#fff"); r(20, 10, 1, 1, "#fff");
      if (f === 1) { r(10, 18, 10, 2, "#7a2e14"); r(19, 18, 10, 1, "#ff5d7a"); r(29, 14, 7, 6, "#fffdf5"); r(30, 16, 5, 1, "#c9c3ad"); r(30, 18, 4, 1, "#c9c3ad"); }
      else { r(10, 18, 10, 1, OL); }
      o.globalAlpha = .7; r(6, 17, 2, 1, "#ff89a8"); r(23, 17, 2, 1, "#ff89a8"); o.globalAlpha = 1;
      r(8, 28, 4, 2, "#4f9f42"); r(19, 28, 4, 2, "#4f9f42"); },
    ember: function (o, h, f) { f = f || 0; var e = h.ell, r = h.rct; // firefly, lit tail
      e(12, 14, 5, 4, OL); e(12, 14, 4, 3, "#5a4a6a"); e(8, 13, 3, 3, "#2b2118"); r(7, 12, 1, 1, "#fff");
      o.globalAlpha = .55; e(13, 9, 4, 3, "#e8f4ff"); e(17, 10, 3, 2, "#e8f4ff"); o.globalAlpha = 1;
      e(18, 16, 4, 3, OL); e(18, 16, 3, 2, f === 1 ? "#fff6a0" : "#ffd84a"); if (f === 1) { r(23, 15, 1, 1, "#fff6a0"); r(17, 20, 1, 1, "#fff6a0"); }
      r(6, 9, 1, 3, "#2b2118"); r(5, 8, 1, 1, "#2b2118"); }
  };
  window.Yappy = { C: C, base: base, drawChick: drawChick, currentPill: currentPill, worldWindow: worldWindow, sprite: sprite, critter: critter, CRITTERS: CRITTERS };
  /* Animate every [data-anim] canvas with the same tick; parks under reduced motion. */
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var jobs = [];
  window.Yappy.loop = function (fn) { jobs.push(fn); };
  window.Yappy.reduce = reduce;
  var t0 = performance.now(), last = 0;
  function tick(ts) {
    if (ts - last > 66) { last = ts; var t = (ts - t0) / 1000; jobs.forEach(function (j) { try { j(t); } catch (e) { } }); }
    requestAnimationFrame(tick);
  }
  window.addEventListener("load", function () { var t = 1.2; jobs.forEach(function (j) { try { j(t); } catch (e) { } }); if (!reduce) requestAnimationFrame(tick); });
})();
