/* Round-two art: every friend redrawn at Yappy's density and shading rules.
 * Rules (same as Yappy): light from top-left, a base tone + a light ellipse +
 * a shade ellipse, a warm dark outline, 1-2 px eye highlights, 0.7 blush, a
 * ground shadow so nobody looks pasted on. Pure canvas, no assets. */
(function () {
  "use strict";
  var Y = window.Yappy;
  function H(o) { return { ell: function (cx, cy, rx, ry, col, a) { o.fillStyle = col; if (a != null) o.globalAlpha = a; for (var y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) { var t = (y + .5 - cy) / ry; if (Math.abs(t) > 1) continue; var hw = rx * Math.sqrt(1 - t * t); o.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1); } o.globalAlpha = 1; },
    r: function (x, y, w, h, col, a) { o.fillStyle = col; if (a != null) o.globalAlpha = a; o.fillRect(Math.round(x), Math.round(y), w, h); o.globalAlpha = 1; } }; }
  function shadow(h, cx, y, rx) { h.ell(cx, y, rx, 1.6, "#3a2418", .2); }
  function eye(h, x, y, f, big) { if (f === 1) { h.r(x - 2, y, 4, 1, "#2b2118"); return; } h.ell(x, y, big ? 2.6 : 2.1, big ? 3 : 2.5, "#2b2118"); h.r(x - 1, y - 2, 2, 1, "#fff"); h.r(x - 1, y - 1, 1, 1, "#fff"); h.r(x + 1, y + 1, 1, 1, "#fff", .7); }
  var D = {};
  /* Yappy, proposed detail pass: rim-lit outline, a shade crescent, a leaf sprout, wing feathers, toes, bigger eye shine. */
  D.yappy = { w: 40, h: 36, draw: function (o, h, f) { var cx = 20, cy = 20; shadow(h, cx, 33, 10);
      h.ell(cx + 10, cy + 2, 4, 3, "#5a2e24"); h.ell(cx + 10, cy + 2, 3, 2, "#e09a1c");
      h.ell(cx, cy, 12, 12, "#5a2e24"); h.ell(cx, cy - .6, 11.6, 11.6, "#9a6248"); h.ell(cx, cy, 11, 11, "#ffd24a");
      h.ell(cx + 3, cy + 4, 8, 7, "#f2b52e"); h.ell(cx - 1, cy - 1, 9, 9, "#ffd24a"); h.ell(cx - 3, cy - 4, 7, 6, "#ffe9a0"); h.r(cx - 7, cy - 8, 3, 1, "#fffbe6"); h.r(cx - 8, cy - 7, 1, 1, "#fffbe6");
      h.ell(cx, cy + 5, 6, 5.5, "#fff2c4"); h.r(cx - 4, cy + 9, 8, 1, "#f3dca0");
      h.r(cx - 1, cy - 13, 1, 3, "#c98a14"); h.r(cx - 3, cy - 14, 2, 2, "#e8a41f"); h.r(cx, cy - 15, 3, 2, "#e8a41f"); h.r(cx + 1, cy - 15, 1, 1, "#ffd27a");
      h.ell(cx - 10, cy + 1, 3, 4.5, "#5a2e24"); h.ell(cx - 10, cy + 1, 2, 3.5, "#ffd24a"); h.r(cx - 11, cy + 3, 2, 1, "#e09a1c"); h.r(cx - 11, cy + 1, 1, 1, "#e09a1c");
      eye(h, cx - 5, cy + 1, f, true); eye(h, cx + 5, cy + 1, f, true);
      h.r(cx - 1, cy + 5, 3, 1, "#e07a12"); h.r(cx - 2, cy + 6, 5, 1, "#ff9f1c"); h.r(cx - 1, cy + 7, 3, 1, f === 2 ? "#7a2e14" : "#ff9f1c"); if (f === 2) h.r(cx, cy + 8, 1, 1, "#ff5d7a");
      h.r(cx - 9, cy + 4, 3, 1, "#ff89a8", .75); h.r(cx + 7, cy + 4, 3, 1, "#ff89a8", .75); h.r(cx - 8, cy + 5, 1, 1, "#ff89a8", .4);
      [-4, 3].forEach(function (x) { h.r(cx + x, cy + 11, 1, 2, "#f2861a"); h.r(cx + x - 1, cy + 13, 3, 1, "#d06a10"); }); } };
  D.hoot = { w: 40, h: 40, draw: function (o, h, f, opt) { var cx = 20, cy = 23; shadow(h, cx, 37, 11);
      h.ell(cx, cy, 13, 13.5, "#4a2c1e"); h.ell(cx, cy, 12, 12.5, "#a8774f"); h.ell(cx + 4, cy + 5, 8, 8, "#8a5d3a"); h.ell(cx - 1, cy - 1, 10, 10, "#a8774f"); h.ell(cx - 4, cy - 5, 6, 5, "#c8956a");
      h.ell(cx, cy + 6, 7.5, 6.5, "#f0dcb4"); [[-4, 4], [0, 5], [4, 4], [-2, 8], [2, 8]].forEach(function (p) { h.r(cx + p[0], cy + p[1], 1, 1, "#c9a878"); h.r(cx + p[0] + 1, cy + p[1] + 1, 1, 1, "#c9a878"); });
      h.r(cx - 10, cy - 14, 3, 5, "#4a2c1e"); h.r(cx - 9, cy - 13, 2, 4, "#a8774f"); h.r(cx + 7, cy - 14, 3, 5, "#4a2c1e"); h.r(cx + 7, cy - 13, 2, 4, "#a8774f");
      h.ell(cx - 5, cy - 3, 5.5, 5, "#4a2c1e"); h.ell(cx + 5, cy - 3, 5.5, 5, "#4a2c1e"); h.ell(cx - 5, cy - 3, 4.6, 4.2, "#fbf1dc"); h.ell(cx + 5, cy - 3, 4.6, 4.2, "#fbf1dc");
      eye(h, cx - 5, cy - 3, f, true); eye(h, cx + 5, cy - 3, f, true);
      h.r(cx - 1, cy, 3, 2, "#ff9f1c"); h.r(cx, cy + 2, 1, 1, "#e07a12");
      h.ell(cx - 12, cy + 3, 3, 7, "#4a2c1e"); h.ell(cx - 12, cy + 3, 2, 6, "#8a5d3a"); h.ell(cx + 12, cy + 3, 3, 7, "#4a2c1e"); h.ell(cx + 12, cy + 3, 2, 6, "#8a5d3a"); h.r(cx - 13, cy + 5, 2, 1, "#6e4428"); h.r(cx + 12, cy + 5, 2, 1, "#6e4428");
      h.r(cx - 10, cy - 4, 1, 1, "#ff89a8", .6); h.r(cx + 9, cy - 4, 1, 1, "#ff89a8", .6);
      [-4, 3].forEach(function (x) { h.r(cx + x - 1, cy + 13, 3, 1, "#f2861a"); h.r(cx + x, cy + 12, 1, 1, "#f2861a"); });
      if (!(opt && opt.bare)) { h.r(cx - 9, cy - 16, 18, 2, "#2a2d35"); h.r(cx - 9, cy - 16, 18, 1, "#5a606e"); h.r(cx - 12, cy - 14, 2, 8, "#2a2d35"); h.r(cx + 10, cy - 14, 2, 8, "#2a2d35"); h.r(cx - 14, cy - 8, 4, 7, "#c8443a"); h.r(cx - 14, cy - 8, 2, 2, "#ff7a6a"); h.r(cx + 10, cy - 8, 4, 7, "#c8443a"); h.r(cx + 12, cy - 8, 2, 2, "#ff7a6a"); } } };
  D.shelly = { w: 46, h: 36, draw: function (o, h, f) { shadow(h, 23, 33, 18);
      h.r(4, 25, 36, 7, "#5a4632"); h.r(5, 25, 35, 6, "#e8d9b5"); h.r(5, 29, 35, 2, "#cdbb92"); h.r(6, 25, 30, 1, "#fff4d8");
      h.ell(37, 20, 5, 7, "#5a4632"); h.ell(37, 20, 4, 6, "#e8d9b5"); h.ell(38, 22, 3, 4, "#cdbb92"); h.ell(36, 18, 2, 3, "#fff4d8");
      h.r(35, 8 - f, 1, 7 + f, "#5a4632"); h.r(40, 8 - f, 1, 7 + f, "#5a4632");
      h.ell(35.5, 7 - f, 2.2, 2.2, "#5a4632"); h.ell(35.5, 7 - f, 1.6, 1.6, "#fff"); h.r(35, 7 - f, 1, 1, "#2b2118"); h.ell(40.5, 7 - f, 2.2, 2.2, "#5a4632"); h.ell(40.5, 7 - f, 1.6, 1.6, "#fff"); h.r(40, 7 - f, 1, 1, "#2b2118");
      h.r(36, 22, 3, 1, "#5a4632"); h.r(35, 21, 1, 1, "#5a4632"); h.r(34, 20, 2, 1, "#ff89a8", .7);
      h.ell(18, 16, 12, 12, "#2f5a2c"); h.ell(18, 16, 11, 11, "#7fbf6a"); h.ell(21, 19, 8, 8, "#5f9f4e"); h.ell(16, 13, 7, 6, "#a7dc90");
      h.ell(18, 16, 7, 7, "#2f5a2c"); h.ell(18, 16, 6, 6, "#7fbf6a"); h.ell(19, 17, 3.5, 3.5, "#2f5a2c"); h.ell(19, 17, 2.5, 2.5, "#a7dc90"); h.r(19, 17, 1, 1, "#2f5a2c");
      h.r(9, 7, 3, 1, "#e8ffd8"); h.r(8, 8, 1, 2, "#e8ffd8");
      h.r(24, 2, 7, 10, "#2a2d35"); h.r(25, 1, 5, 1, "#2a2d35"); h.r(25, 3, 5, 8, "#3b3f4a"); var lv = f === 1 ? 3 : 2; for (var i = 0; i < lv; i++) h.r(26, 9 - i * 3, 3, 2, i === lv - 1 ? "#ffd23f" : "#7fdc7a"); } };
  D.tidy = { w: 42, h: 36, draw: function (o, h, f) { shadow(h, 20, 33, 13);
      h.r(31 - f, 6, 1, 22, "#6b4428"); h.r(32 - f, 6, 1, 22, "#a5704a"); h.r(27 - f, 27, 9, 5, "#9a7a3a"); h.r(28 - f, 27, 7, 4, "#e8c86a"); for (var k = 0; k < 4; k++) h.r(28 - f + k * 2, 31, 1, 2, "#c9a45c");
      h.ell(20, 21, 12, 10, "#5a1a14"); h.ell(20, 21, 11, 9, "#e0453a"); h.ell(23, 24, 8, 6, "#b8332a"); h.ell(17, 17, 6, 4, "#ff7a6a"); h.r(14, 15, 3, 1, "#ffd0c8");
      h.r(20, 12, 1, 18, "#5a1a14"); [[14, 20, 2.4], [26, 19, 2.4], [15, 26, 1.8], [25, 26, 1.8], [20, 23, 1.6]].forEach(function (s) { if (s[0] === 20) return; h.ell(s[0], s[1], s[2], s[2], "#2b2118"); h.r(s[0] - 1, s[1] - 1, 1, 1, "#5a4a4a"); });
      h.ell(20, 11, 7, 5.5, "#2b2118"); h.ell(18, 10, 3, 2, "#4a3a3a"); h.ell(16, 11, 2.6, 2.8, "#fff"); h.ell(24, 11, 2.6, 2.8, "#fff");
      if (f === 1) { h.r(15, 11, 3, 1, "#2b2118"); h.r(23, 11, 3, 1, "#2b2118"); } else { h.r(16, 11, 2, 2, "#2b2118"); h.r(24, 11, 2, 2, "#2b2118"); h.r(16, 11, 1, 1, "#fff"); h.r(24, 11, 1, 1, "#fff"); }
      h.r(18, 14, 4, 1, "#ff89a8", .6); h.r(15, 4, 1, 4, "#2b2118"); h.r(14, 3, 1, 1, "#2b2118"); h.r(25, 4, 1, 4, "#2b2118"); h.r(26, 3, 1, 1, "#2b2118");
      [12, 16, 24, 28].forEach(function (x) { h.r(x, 30, 1, 2, "#2b2118"); }); } };
  D.gulp = { w: 46, h: 38, draw: function (o, h, f) { shadow(h, 21, 35, 14);
      h.ell(21, 24, 14, 10, "#2f5a2c"); h.ell(21, 24, 13, 9, "#6cc35a"); h.ell(24, 27, 10, 6, "#4f9f42"); h.ell(18, 20, 9, 5, "#94dc7c"); h.ell(21, 28, 8, 5, "#e1f4c4"); h.r(15, 32, 12, 1, "#c4dca0");
      [[14, 13], [28, 13]].forEach(function (p) { h.ell(p[0], p[1], 5, 5, "#2f5a2c"); h.ell(p[0], p[1], 4, 4, "#6cc35a"); h.ell(p[0] - 1, p[1] - 1, 2, 2, "#94dc7c"); h.ell(p[0], p[1] + 1, 2.6, 2.6, "#fff"); });
      eye(h, 14, 14, f === 1 ? 0 : 0); eye(h, 28, 14, 0);
      if (f === 1) { h.r(12, 21, 18, 3, "#7a2e14"); h.r(13, 22, 16, 1, "#ff5d7a"); h.r(29, 22, 9, 1, "#ff5d7a"); h.r(37, 17, 9, 8, "#c9c3ad"); h.r(38, 18, 7, 6, "#fffdf5"); h.r(39, 20, 5, 1, "#8b877f"); h.r(39, 22, 4, 1, "#8b877f"); }
      else { h.r(12, 21, 18, 1, "#2f5a2c"); h.r(11, 20, 1, 1, "#2f5a2c"); h.r(30, 20, 1, 1, "#2f5a2c"); }
      h.r(9, 23, 3, 1, "#ff89a8", .7); h.r(31, 23, 3, 1, "#ff89a8", .7);
      [[9, 33], [29, 33]].forEach(function (p) { h.r(p[0], p[1], 6, 2, "#4f9f42"); h.r(p[0], p[1] + 1, 1, 1, "#2f5a2c"); h.r(p[0] + 2, p[1] + 1, 1, 1, "#2f5a2c"); h.r(p[0] + 4, p[1] + 1, 1, 1, "#2f5a2c"); }); } };
  D.ember = { w: 32, h: 28, draw: function (o, h, f) {
      h.ell(13, 8, 6, 4, "#9fb8d8"); h.ell(13, 8, 5, 3, "#eef6ff", .75); h.ell(19, 9, 4, 3, "#9fb8d8"); h.ell(19, 9, 3, 2, "#eef6ff", .75);
      h.ell(14, 15, 6, 4.5, "#2e2440"); h.ell(14, 15, 5, 3.5, "#5a4a6a"); h.ell(13, 14, 3, 2, "#7a6a8a");
      h.ell(8, 14, 4, 4, "#2e2440"); h.ell(8, 14, 3.2, 3.2, "#3e3450"); h.ell(7, 14, 1.6, 1.8, "#fff"); h.r(7, 14, 1, 1, "#2b2118"); h.r(4, 9, 1, 3, "#2e2440"); h.r(3, 8, 1, 1, "#2e2440"); h.r(6, 9, 1, 2, "#2e2440");
      h.ell(21, 18, 5, 4, "#8a6a10"); h.ell(21, 18, 4, 3, f === 1 ? "#fff6a0" : "#ffd84a"); h.ell(20, 17, 2, 1.5, "#fffbe0");
      if (f === 1) { [[27, 16], [26, 21], [17, 23], [24, 23], [27, 19]].forEach(function (p) { h.r(p[0], p[1], 1, 1, "#fff6a0", .55); }); }
      h.r(11, 19, 1, 2, "#2e2440"); h.r(15, 19, 1, 2, "#2e2440"); } };
  D.sal = { w: 46, h: 30, draw: function (o, h, f) { shadow(h, 22, 27, 18);
      h.r(30, 18 + f, 12, 3, "#7a2e14"); h.r(31, 18 + f, 11, 2, "#ff8a3c"); h.r(40, 17 + f, 4, 2, "#ff8a3c");
      h.ell(22, 18, 11, 6, "#7a2e14"); h.ell(22, 18, 10, 5, "#ff7a3c"); h.ell(24, 20, 8, 3, "#e05a24"); h.ell(20, 16, 7, 2.5, "#ffa86a");
      [16, 21, 26].forEach(function (x) { h.r(x, 15, 2, 2, "#ffd23f"); h.r(x, 15, 1, 1, "#fff2b0"); });
      h.ell(9, 16, 7, 5.5, "#7a2e14"); h.ell(9, 16, 6, 4.5, "#ff7a3c"); h.ell(8, 14, 4, 2.5, "#ffa86a"); eye(h, 7, 15, f); eye(h, 12, 15, f); h.r(5, 19, 6, 1, "#7a2e14"); h.r(4, 18, 1, 1, "#7a2e14"); h.r(4, 17, 2, 1, "#ff89a8", .7); h.r(13, 17, 2, 1, "#ff89a8", .7);
      [[13, 22], [17, 22], [27, 22], [31, 22]].forEach(function (p) { h.r(p[0], p[1], 2, 3, "#e05a24"); h.r(p[0] - 1, p[1] + 3, 4, 1, "#7a2e14"); }); } };
  D.mochi = { w: 40, h: 40, draw: function (o, h, f) { shadow(h, 20, 37, 11);
      h.ell(20, 27, 11, 10, "#3e3e48"); h.ell(20, 27, 10, 9, "#9a9aa6"); h.ell(23, 30, 7, 6, "#7c7c88"); h.ell(20, 30, 6, 5, "#dcdce4");
      h.ell(8, 12, 6, 6, "#3e3e48"); h.ell(8, 12, 5, 5, "#9a9aa6"); h.ell(8, 12, 3, 3, "#f0c8d0"); h.ell(32, 12, 6, 6, "#3e3e48"); h.ell(32, 12, 5, 5, "#9a9aa6"); h.ell(32, 12, 3, 3, "#f0c8d0");
      h.ell(20, 15, 11, 9, "#3e3e48"); h.ell(20, 15, 10, 8, "#9a9aa6"); h.ell(17, 12, 6, 4, "#b4b4c0");
      eye(h, 15, 15, f ? 1 : 1); eye(h, 25, 15, 1);
      h.ell(20, 18, 3, 3.5, "#2b2b33"); h.r(19, 16, 1, 1, "#6a6a78"); h.r(12, 19, 3, 1, "#ff89a8", .7); h.r(25, 19, 3, 1, "#ff89a8", .7);
      h.r(14, 24, 4, 3, "#7c7c88"); h.r(22, 24, 4, 3, "#7c7c88"); h.r(17, 23, 1, 1, "#3e3e48"); h.r(22, 23, 1, 1, "#3e3e48");
      if (f === 1) { h.r(30, 3, 2, 1, "#5a6280"); h.r(31, 4, 1, 1, "#5a6280"); h.r(30, 5, 2, 1, "#5a6280"); } } };
  /* Cinder, redrawn from Wilson's 2026-11-06 reference: a round red chibi
   * dragon, cream belly and horns, small pink-membrane wings, big eyes, a
   * little flame on the chest. f: 0 idle, 1 blink. opt.talk opens the mouth. */
  D.cinder = { w: 46, h: 42, draw: function (o, h, f, opt) { var cx = 23, talk = opt && opt.talk, fl = Math.floor((opt && opt.t || 0) * 6) % 2;
      shadow(h, cx, 39, 12);
      // tail with a spade tip
      h.r(31, 31, 6, 3, "#6a1420"); h.r(32, 31, 5, 2, "#d8343a"); h.r(36, 28, 3, 4, "#6a1420"); h.r(37, 29, 2, 2, "#d8343a"); h.r(38, 26, 4, 4, "#6a1420"); h.r(39, 27, 2, 2, "#ff6b6b");
      // wings, behind
      h.r(5, 15, 3, 3, "#6a1420"); h.r(38, 15, 3, 3, "#6a1420");
      h.ell(9, 20, 5, 4.5, "#6a1420"); h.ell(9, 20, 4, 3.5, "#f28aa0"); h.r(7, 18, 1, 5, "#c85a72"); h.r(10, 18, 1, 5, "#c85a72"); h.r(6, 16, 2, 2, "#d8343a");
      h.ell(37, 20, 5, 4.5, "#6a1420"); h.ell(37, 20, 4, 3.5, "#f28aa0"); h.r(35, 18, 1, 5, "#c85a72"); h.r(38, 18, 1, 5, "#c85a72"); h.r(38, 16, 2, 2, "#d8343a");
      // body
      h.ell(cx, 29, 10.5, 9, "#6a1420"); h.ell(cx, 29, 9.5, 8, "#e0393e"); h.ell(cx + 3, 32, 7, 5, "#b82a33"); h.ell(cx - 3, 26, 5, 3, "#ff6b6b");
      h.ell(cx, 31, 6, 6, "#e8cfa0"); h.ell(cx, 30.5, 5.4, 5.4, "#fbe8c4"); [29, 32, 35].forEach(function (y) { h.r(cx - 4, y, 8, 1, "#ead2a2"); });
      // chest flame
      h.r(cx - 1, 25 - fl, 2, 1, "#ffd23f"); h.r(cx - 2, 26, 4, 2, "#ff8a3c"); h.r(cx - 1, 26, 2, 2, "#ffd23f"); h.r(cx - 1, 28, 2, 1, "#ff8a3c"); h.r(cx, 27, 1, 1, "#fff6c0");
      // arms and feet
      h.ell(13, 29, 2.5, 2, "#6a1420"); h.ell(13, 29, 1.6, 1.2, "#e0393e"); h.ell(33, 29, 2.5, 2, "#6a1420"); h.ell(33, 29, 1.6, 1.2, "#e0393e");
      [[17, 37], [29, 37]].forEach(function (p) { h.ell(p[0], p[1], 3.5, 2, "#6a1420"); h.ell(p[0], p[1], 2.6, 1.3, "#d8343a"); h.r(p[0] - 2, p[1] + 1, 1, 1, "#fbe8c4"); h.r(p[0], p[1] + 1, 1, 1, "#fbe8c4"); h.r(p[0] + 2, p[1] + 1, 1, 1, "#fbe8c4"); });
      // horns and head spikes
      h.r(12, 5, 3, 5, "#7a5a3a"); h.r(13, 5, 2, 4, "#f6e3b0"); h.r(11, 3, 2, 3, "#7a5a3a"); h.r(12, 3, 1, 2, "#f6e3b0");
      h.r(31, 5, 3, 5, "#7a5a3a"); h.r(31, 5, 2, 4, "#f6e3b0"); h.r(33, 3, 2, 3, "#7a5a3a"); h.r(33, 3, 1, 2, "#f6e3b0");
      // head
      h.ell(cx, 14, 12, 10, "#6a1420"); h.ell(cx, 14, 11, 9, "#e0393e"); h.ell(cx + 4, 17, 8, 5, "#c82e36"); h.ell(cx - 2, 12, 8, 6, "#e0393e"); h.ell(cx - 5, 9, 4, 2.5, "#ff6b6b"); h.r(cx - 8, 7, 2, 1, "#ffd0d0");
      [[cx - 3, 4], [cx, 3], [cx + 3, 4]].forEach(function (p) { h.r(p[0] - 1, p[1], 3, 2, "#6a1420"); h.r(p[0], p[1], 1, 1, "#f6e3b0"); });
      // eyes: big, dark, double shine
      if (f === 1) { h.r(cx - 8, 15, 5, 1, "#2b2118"); h.r(cx + 3, 15, 5, 1, "#2b2118"); }
      else { [cx - 5.5, cx + 5.5].forEach(function (x) { h.ell(x, 14, 3.2, 3.8, "#2b2118"); h.ell(x, 15, 2.2, 2, "#3a3a6a"); h.r(x - 2, 12, 2, 2, "#fff"); h.r(x + 1, 16, 1, 1, "#fff"); }); }
      // snout, nostrils, mouth
      h.ell(cx, 19, 4.5, 2.5, "#ff7b7b"); h.r(cx - 2, 18, 1, 1, "#8a1a24"); h.r(cx + 1, 18, 1, 1, "#8a1a24");
      if (talk) { h.r(cx - 2, 20, 4, 3, "#6a1420"); h.r(cx - 1, 22, 2, 1, "#ff5d7a"); } else { h.r(cx - 2, 21, 1, 1, "#6a1420"); h.r(cx - 1, 22, 2, 1, "#6a1420"); h.r(cx + 1, 21, 1, 1, "#6a1420"); }
      h.r(cx - 11, 18, 3, 1, "#ff89a8"); h.r(cx + 8, 18, 3, 1, "#ff89a8");
    } };
  /* A tiny heart speech bubble, drawn next to a character. */
  function heart(o, x, y) { var h = H(o); h.r(x, y, 13, 11, "#3a2418", .25); h.r(x - 1, y - 1, 13, 11, "#6a5a4a"); h.r(x, y, 11, 9, "#fffdf5"); h.r(x + 2, y + 9, 2, 2, "#fffdf5"); h.r(x + 1, y + 10, 1, 1, "#6a5a4a");
    h.r(x + 3, y + 2, 2, 1, "#e5484d"); h.r(x + 6, y + 2, 2, 1, "#e5484d"); h.r(x + 2, y + 3, 7, 2, "#e5484d"); h.r(x + 3, y + 5, 5, 1, "#e5484d"); h.r(x + 4, y + 6, 3, 1, "#e5484d"); h.r(x + 5, y + 7, 1, 1, "#e5484d"); h.r(x + 3, y + 3, 1, 1, "#ffb0b0"); }
  function paint(cv, name, f, opt) { var d = D[name]; Y.critter(cv, function (o) { d.draw(o, H(o), f || 0, opt); }, d.w, d.h); }
  function stamp(o, name, x, y, f, opt) { o.save(); o.translate(Math.round(x), Math.round(y)); D[name].draw(o, H(o), f || 0, opt); o.restore(); }
  /* The studio: 256 x 112 pixels, every friend at their station. */
  function studio(cv, t) {
    var W = 320, Hh = 112, b = document.createElement("canvas"); b.width = W; b.height = Hh; var o = b.getContext("2d"), h = H(o), fr = Math.floor(t * 2) % 2;
    h.r(0, 0, W, 84, "#eccf92"); for (var i = 0; i < 14; i++) h.r(0, i, W, 1, "#c39a5f", .35 * (1 - i / 14)); h.r(0, 81, W, 3, "#d8b476");
    h.r(0, 84, W, 28, "#a5673f"); h.r(0, 84, W, 1, "#bd8151"); for (var x = 0; x < W; x += 18) h.r(x, 86, 1, 26, "#6f3f27");
    // Ember's window (night)
    h.r(8, 14, 40, 36, "#5f3c22"); h.r(10, 16, 36, 32, "#171a3d"); h.r(10, 36, 36, 12, "#2c3060"); h.r(27, 16, 2, 32, "#5f3c22"); h.r(10, 31, 36, 2, "#5f3c22"); h.ell(40, 22, 3, 3, "#f6f0cf");
    [[14, 20], [20, 26], [35, 40], [16, 42]].forEach(function (p) { h.r(p[0], p[1], 1, 1, "#fffbe0"); }); h.r(6, 50, 44, 3, "#a5704a");
    o.save(); o.translate(20, 22); o.scale(.5, .5); D.ember.draw(o, H(o), fr); o.restore(); o.save(); o.translate(32, 36); o.scale(.5, .5); D.ember.draw(o, H(o), 1 - fr); o.restore();
    // Hoot's meeting booth
    h.r(56, 30, 44, 54, "#3b3f4a"); h.r(58, 32, 40, 50, "#4a4f5c"); for (var k = 0; k < 5; k++) h.r(60, 36 + k * 9, 36, 1, "#3b3f4a"); h.r(68, 24, 20, 6, "#2a2d35"); h.r(70, 25, 16, 4, fr ? "#e5484d" : "#a8342f"); h.r(72, 26, 12, 1, "#ffb0b0", fr ? .8 : .2);
    h.r(58, 68, 40, 4, "#8a5a36"); h.r(58, 68, 40, 1, "#a5704a"); h.r(62, 72, 2, 12, "#5f3c22"); h.r(92, 72, 2, 12, "#5f3c22"); h.r(90, 52, 1, 16, "#2a2d35"); h.r(88, 50, 5, 4, "#2a2d35");
    stamp(o, "hoot", 58, 30, fr === 1 && (t % 4) < .3 ? 1 : 0);
    // Yappy's typewriter desk (center)
    h.r(106, 64, 46, 5, "#8a5a36"); h.r(106, 64, 46, 1, "#a5704a"); h.r(110, 69, 2, 15, "#5f3c22"); h.r(146, 69, 2, 15, "#5f3c22");
    h.r(128, 54, 18, 10, "#7d8c96"); h.r(128, 54, 18, 2, "#9fb0ba"); h.r(131, 58, 12, 3, "#586771"); h.r(133, 49, 10, 5, "#fff6e6");
    stamp(o, "yappy", 104, 30, (t % 3.4) < .15 ? 1 : (Math.floor(t * 6) % 3 === 0 ? 2 : 0));
    // Tidy's workbench
    h.r(158, 58, 30, 4, "#6b4428"); h.r(158, 58, 30, 1, "#8a5a36"); h.r(160, 62, 2, 22, "#5f3c22"); h.r(184, 62, 2, 22, "#5f3c22"); h.r(160, 36, 26, 2, "#8a5a36");
    h.r(163, 30, 2, 6, "#7d8c96"); h.r(168, 28, 2, 8, "#c8443a"); h.r(173, 31, 6, 5, "#e8c86a"); h.r(181, 29, 2, 7, "#3fb0a0"); stamp(o, "tidy", 160, 24, fr);
    // Gulp's word jar
    h.r(194, 50, 18, 34, "#9fc8d0", .55); h.r(194, 50, 18, 1, "#ffffff", .8); h.r(193, 48, 20, 3, "#8a5a36"); h.r(195, 52, 1, 30, "#ffffff", .5);
    [[197, 74, "#fffdf5"], [202, 70, "#fff2b0"], [206, 76, "#fffdf5"], [199, 64, "#f6e3b0"], [205, 60, "#fffdf5"]].forEach(function (p) { h.r(p[0], p[1], 5, 3, p[2]); h.r(p[0] + 1, p[1] + 1, 3, 1, "#8b877f"); });
    stamp(o, "gulp", 208, 48, fr && (t % 5) < 1 ? 1 : 0);
    // Shelly's charging nook + Sal's furnace door
    h.r(278, 62, 6, 8, "#e7e1d0"); h.r(280, 64, 1, 2, "#3b3f4a"); h.r(282, 64, 1, 2, "#3b3f4a"); h.r(272, 68, 6, 1, "#2a2d35"); h.r(271, 58, 1, 11, "#2a2d35");
    stamp(o, "shelly", 236, 50, fr);
    h.r(290, 30, 26, 54, "#5f3c22"); h.r(292, 32, 22, 52, "#3a2418"); h.ell(303, 70, 7, 6, fr ? "#ff8a3c" : "#ffb04a"); h.ell(303, 71, 4, 3, "#ffe27a"); h.r(292, 32, 22, 2, "#7a4b2e"); o.save(); o.translate(292, 74); o.scale(.5, .5); D.sal.draw(o, H(o), fr); o.restore();
    o.save(); o.translate(291, 8); o.scale(.6, .6); D.mochi.draw(o, H(o), 1); o.restore();
    // vignette-free: a soft ambient shade on the floor edge
    h.r(0, 110, W, 2, "#6f3f27");
    var s = cv.getContext("2d"); var sc = Math.max(1, Math.floor(cv.width / W)); cv.height = Hh * sc; s.imageSmoothingEnabled = false; s.drawImage(b, 0, 0, W * sc, Hh * sc);
  }
  window.FR = { D: D, paint: paint, stamp: stamp, studio: studio, H: H, heart: heart };
})();
