/* Pill page, section 16: meetings as a film crew (option C), next to A and B.
 * Design only. Draws with the approved art kit (yappy.js, pill-kit.js,
 * friends-hd.js, pill-r3.js, copied verbatim from design-2026-11-06/assets) and
 * adds the film gear: a tripod camera whose REC light is the recording signal,
 * Hoot's boom mic, a clapperboard for the start and a "NOTES READY" slate for
 * the stop. Drop-in: needs only #fcRows and the four globals below.
 * ?still=1 (or reduce motion) pins every frame to one moment so the headless
 * PNGs (render.mjs) are the same on every run. */
(function () {
  "use strict";
  var Y = window.Yappy, X = PK.X, I = PK.icon, R3 = window.R3;
  var STILL = /[?&]still=1/.test(location.search) || !!Y.reduce;
  function T(t) { return STILL ? 0.5 : t; }
  var blink = function (t) { return (t % 3.6) < 0.13 ? 1 : 0; };
  var talk = function (t) { return [0, 1, 2, 1, 0, 2][Math.floor(t * 7) % 6]; };
  var hop = function (t, a) { var p = t % 2.4; return p < 0.3 ? Math.round(Math.sin(p / 0.3 * Math.PI) * (a || 4)) : 0; };
  var recOn = function (t) { return (t % 1.6) < 1.1; };
  var stopBtn = '<button class="sbtn red" aria-label="Stop">' + I("stop", 14) + '</button>';
  var savedSeg = I("check", 14) + '<span>Saved</span>';
  var readySeg = I("check", 14) + '<span>Notes ready</span><button class="cbtn">Open</button>';
  function sunX(min) { return 32 + Math.min(min, 60) / 60 * 116; }
  /* The approved meeting clock: the sun crosses the sky, dusk after an hour. */
  function sky(min, a) { a.world = 1; a.sunX = sunX(min); a.sunY = 11; a.sunCol = "#ffd23f"; if (min >= 60) { a.skyTop = "#ff9d6b"; a.skyHi = "#ffd27a"; } return a; }

  /* ---- film gear (scene pixels; the meeting window shows x 18..158, ground at y 30) */
  /* Tripod camera. Its REC light is THE recording light in option C. */
  function camera(o, x, feet, t, off) { var h = R3.H(o);
    h.rct(x + 2, feet - 8, 1, 8, "#2a2d35"); h.rct(x - 1, feet - 1, 2, 1, "#2a2d35"); h.rct(x + 5, feet - 1, 2, 1, "#2a2d35"); h.rct(x, feet - 4, 1, 4, "#2a2d35"); h.rct(x + 4, feet - 4, 1, 4, "#2a2d35");
    h.rct(x - 4, feet - 16, 13, 8, "#1e2025"); h.rct(x - 3, feet - 15, 11, 6, "#3b3f4a"); h.rct(x - 3, feet - 15, 11, 1, "#5a606e");
    h.rct(x + 9, feet - 14, 4, 4, "#1e2025"); h.rct(x + 10, feet - 13, 2, 2, "#6a9ad8");
    h.rct(x - 2, feet - 20, 4, 4, "#1e2025"); h.rct(x + 3, feet - 20, 4, 4, "#1e2025"); h.rct(x - 1, feet - 19, 2, 2, "#5a606e"); h.rct(x + 4, feet - 19, 2, 2, "#5a606e");
    h.rct(x + 6, feet - 14, 2, 2, off ? "#4a2f2c" : (recOn(t) ? "#ff3b3b" : "#7a2a24")); if (!off && recOn(t)) h.rct(x + 6, feet - 14, 1, 1, "#ffd0d0"); }
  /* Boom out over the scene: Hoot (sprite at hx) holds the pole, the mic hangs over tx. Sways 1 px every 4 s (never in still/reduce motion). */
  function boomOut(o, hx, feet, tx, t) { var h = R3.H(o), sw = STILL ? 0 : Math.floor(t / 4) % 2, y = feet - 24 + sw, px = hx + 18;
    /* Level arm at y: the meeting window crops the scene above y 4, so the arm sits low enough to read at true size. */
    h.rct(px, y, 2, feet - 12 - y, "#4a5060"); h.rct(px, y, 1, feet - 12 - y, "#8a93a6");
    h.rct(px, y - 1, tx - px + 1, 1, "#9aa3ad"); h.rct(px, y, tx - px + 1, 1, "#4a5060");
    h.rct(tx, y + 1, 1, 1, "#2a2d35"); h.ell(tx, y + 3, 4, 2, "#6a6a6a"); h.ell(tx, y + 2, 3, 1, "#8a8a8a"); h.rct(tx - 3, y + 3, 1, 1, "#a8a8a8"); h.rct(tx + 2, y + 4, 1, 1, "#5a5a5a"); }
  /* Boom at rest: held upright beside Hoot (before the clap, and after stop). */
  function boomRest(o, hx, feet) { var h = R3.H(o), x = hx + 20; h.rct(x, feet - 25, 2, 13, "#4a5060"); h.rct(x, feet - 25, 1, 13, "#8a93a6"); h.ell(x + 1, feet - 27, 3, 3, "#6a6a6a"); h.ell(x + 1, feet - 28, 2, 2, "#8a8a8a"); h.rct(x - 1, feet - 28, 1, 1, "#a8a8a8"); }
  /* Clapperboard, 13 x 14 px; the striped stick is open (about to snap) or shut. */
  function clapper(o, x, y, open) { var h = R3.H(o);
    h.rct(x, y + 4, 13, 8, "#1e2025"); h.rct(x + 1, y + 5, 11, 6, "#2a2d35");
    h.rct(x + 2, y + 6, 7, 1, "#e8e6e1"); h.rct(x + 2, y + 8, 5, 1, "#e8e6e1"); h.rct(x + 2, y + 10, 8, 1, "#9aa3ad");
    for (var i = 0; i < 13; i++) { var yy = open ? y + 2 - Math.round(i * 0.3) : y + 2; h.rct(x + i, yy, 1, 2, (Math.floor(i / 2) % 2) ? "#f4f2ec" : "#1e2025"); }
    h.rct(x, y + 2, 1, 2, "#8a93a6"); }
  /* 3 x 5 chalk letters for the slate. */
  var GL = { N: ["101", "111", "111", "101", "101"], O: ["111", "101", "101", "101", "111"], T: ["111", "010", "010", "010", "010"], E: ["111", "100", "110", "100", "111"], S: ["111", "100", "111", "001", "111"], R: ["110", "101", "110", "101", "101"], A: ["010", "101", "111", "101", "101"], D: ["110", "101", "101", "101", "110"], Y: ["101", "101", "010", "010", "010"] };
  function word(h, s, x, y, col) { for (var i = 0; i < s.length; i++) { var g = GL[s[i]]; for (var r = 0; r < 5; r++) for (var c = 0; c < 3; c++) if (g[r][c] === "1") h.rct(x + i * 4 + c, y + r, 1, 1, col); } }
  /* The "NOTES READY" slate, 25 x 15 px on a stick planted in front of the lens. */
  function slate(o, x, y, feet) { var h = R3.H(o);
    h.rct(x + 11, y + 15, 3, feet - y - 15, "#6b4428"); h.rct(x + 11, y + 15, 1, feet - y - 15, "#8a5a36");
    h.rct(x, y, 25, 15, "#8a5a36"); h.rct(x + 1, y + 1, 23, 13, "#2a2d35"); h.rct(x + 1, y + 1, 23, 1, "#3b3f4a");
    word(h, "NOTES", x + 3, y + 2, "#f4f2ec"); word(h, "READY", x + 3, y + 8, "#ffd23f"); }
  function mic(o, x, feet) { var h = R3.H(o); h.rct(x, feet - 12, 1, 12, "#2a2d35"); h.ell(x, feet - 14, 2, 3, "#4a5060"); h.rct(x - 1, feet - 15, 1, 1, "#9aa3ad"); h.rct(x - 3, feet - 1, 7, 1, "#2a2d35"); }
  function cassette(o, x, y) { var h = R3.H(o); h.rct(x, y, 8, 5, "#2a2d35"); h.rct(x + 1, y + 1, 6, 3, "#e8e6e1"); }
  var pad = function (t) { return { prop: "pad", propIn: 1, pencil: true, look: "down", beakF: talk(t) }; };
  function mix(a, b) { for (var k in b) a[k] = b[k]; return a; }

  /* ---- the four options. Each returns A(t) for one moment. m = "start" | minutes | "dict" | "stop" */
  var OPT = {
    /* A · Hoot takes the desk (section 9) */
    A: function (m) { return function (t) {
      if (m === "start") return sky(0, { cxOff: -26, blink: blink(t), mood: 1, extra: X.headphones, after: function (o, cx, feet) { R3.hoot(o, 82, feet - 24, blink(t), 24); } });
      if (m === "dict") return sky(14, { cxOff: 34, prop: "pad", propIn: 1, pencil: true, look: "down", beakF: talk(t), before: function (o, cx, feet) { R3.hoot(o, 58, feet - 24, blink(t), 24, true, t); } });
      if (m === "stop") return { world: 1, mood: 1, hop: hop(t), after: function (o, cx, feet) { R3.hoot(o, 48, feet - 24, 0, 24); var h = R3.H(o); h.rct(68, feet - 14, 8, 5, "#2a2d35"); h.rct(69, feet - 13, 6, 3, "#e8e6e1"); } };
      return sky(m, { hideChick: true, after: function (o, cx, feet) { R3.hoot(o, 76, feet - 24, blink(t), 24, true, t); mic(o, 104, feet); } });
    }; },
    /* B · Hoot on the tape beside Yappy (approved) */
    B: function (m) { return function (t) {
      var base = function (min) { return sky(min, { blink: blink(t), extra: R3.recPhones(t), before: function (o, cx, feet) { R3.hoot(o, 26, feet - 24, blink(t + 1), 24); R3.tape(o, 52, feet - 17, t); } }); };
      if (m === "start") return mix(base(0), { hop: hop(t, 2) });
      if (m === "dict") return mix(base(14), pad(t));
      if (m === "stop") return { world: 1, mood: 1, hop: hop(t), before: function (o, cx, feet) { R3.hoot(o, 26, feet - 24, 0, 24); cassette(o, 52, feet - 22); } };
      return base(m);
    }; },
    /* C · Film crew: Yappy on the camera, Hoot on the boom, REC on the camera */
    C: function (m) { return function (t) {
      var min = m === "start" ? 0 : m === "dict" ? 14 : m === "stop" ? 45 : m;
      var a = sky(min, { blink: blink(t), cxOff: m === "dict" ? -18 : -4,
        before: function (o, cx, feet) { R3.hoot(o, 26, feet - 24, m === "stop" ? 0 : blink(t + 1), 24); if (m === "start" || m === "stop") boomRest(o, 26, feet); },
        after: function (o, cx, feet) { camera(o, 98, feet, t, m === "stop");
          if (m === "start") clapper(o, 116, feet - 18, STILL || (t % 1.2) < 0.6);
          else if (m === "stop") slate(o, 116, feet - 25, feet);
          else boomOut(o, 26, feet, cx + 2, t); } });
      if (m === "dict") mix(a, pad(t));
      if (m === "stop") mix(a, { mood: 1, hop: hop(t), blink: 0 });
      return a;
    }; },
    /* C on top of B: Hoot runs the tape AND holds the boom; camera beside Yappy */
    BC: function (m) { return function (t) {
      var min = m === "start" ? 0 : m === "dict" ? 14 : m === "stop" ? 45 : m;
      var stop = m === "stop";
      var a = sky(min, { blink: blink(t), cxOff: m === "dict" ? -5 : 2, extra: stop ? null : R3.recPhones(t),
        before: function (o, cx, feet) { R3.hoot(o, 26, feet - 24, stop ? 0 : blink(t + 1), 24); if (stop) cassette(o, 52, feet - 22); else R3.tape(o, 52, feet - 17, t); if (m === "start" || stop) boomRest(o, 26, feet); },
        after: function (o, cx, feet) { camera(o, 104, feet, t, stop);
          if (m === "start") clapper(o, 122, feet - 18, STILL || (t % 1.2) < 0.6);
          else if (stop) slate(o, 122, feet - 25, feet);
          else boomOut(o, 26, feet, cx + 2, t); } });
      if (m === "start") mix(a, { hop: hop(t, 2) });
      if (m === "dict") mix(a, pad(t));
      if (stop) mix(a, { mood: 1, hop: hop(t), blink: 0 });
      return a;
    }; }
  };

  /* ---- rows: [key, label, note, moment, right segment per option, time label, sun minute, captions A/B/C/BC] */
  var ROWS = [
    ["start", "Start", "You click Record meeting. The pill docks top-center.", "start", { A: stopBtn, B: stopBtn, C: stopBtn, BC: stopBtn }, "0:00", 0,
      ["Yappy hands Hoot the headphones and steps off.", "Hoot drops in and presses record; the right ear cup lights red.", "The clapper snaps once in front of the lens. Hoot holds the boom upright, ready.", "Hoot presses record on the tape; the clapper snaps in front of the lens."]],
    ["1min", "1:00", "The first minute. Nothing talks; the scene settles.", 1, { A: stopBtn, B: stopBtn, C: stopBtn, BC: stopBtn }, "1:00", 1,
      ["Hoot alone at the desk mic; his ear cup is the light.", "Reels turn, the cup glows. Yappy listens.", "Hoot swings the boom out over the scene. Yappy is behind the camera; its REC light blinks.", "Boom out, camera rolling, reels turning. Two red lights now."]],
    ["10min", "10:00", "The sun is a sixth of the way across.", 10, { A: stopBtn, B: stopBtn, C: stopBtn, BC: stopBtn }, "10:00", 10,
      ["Same scene; only the sun has moved.", "Same; one redraw a minute.", "Same; REC light, boom and sun. One redraw a minute.", "Same, and busy at true size."]],
    ["1h", "1:00:00", "Dusk after an hour, as you picked.", 60, { A: stopBtn, B: stopBtn, C: stopBtn, BC: stopBtn }, "1:00:00", 60,
      ["Dusk sky behind Hoot.", "Dusk sky; the tape keeps turning.", "Dusk sky; the camera is still rolling.", "Dusk sky; everything still rolling."]],
    ["dict", "Dictating mid-meeting", "You hold fn to dictate something else. The meeting keeps recording.", "dict", { A: '<span class="kbd">fn</span>' + stopBtn, B: '<span class="kbd">fn</span>' + stopBtn, C: '<span class="kbd">fn</span>' + stopBtn, BC: '<span class="kbd">fn</span>' + stopBtn }, "14:02", 14,
      ["Yappy pops back in with his notepad.", "Yappy takes notes; the tape keeps running.", "Yappy steps out from the camera with his notepad. The camera keeps its REC light; Hoot follows Yappy with the boom.", "Yappy takes notes in his seat; camera and tape keep rolling."]],
    ["stop", "Stop", "You click stop. The notes are saved.", "stop", { A: savedSeg, B: savedSeg, C: readySeg, BC: readySeg }, null, 45,
      ["Hoot hands Yappy the tape. Back to the nub.", "Hoot pops the tape out and waves.", "REC goes dark, Hoot rests the boom, and the NOTES READY slate goes up in front of the lens. Open goes to the notes.", "Tape out, REC dark, boom down, the slate goes up."]]
  ];
  var COLS = ["A", "B", "C", "BC"];

  function cell(A, right, min, label, cap) {
    var d = document.createElement("div"); d.className = "fc-cell";
    var W = function (t) { return A(T(t)); };
    var s = document.createElement("div"); s.className = "screen"; s.innerHTML = '<div class="notch"></div><div class="text"></div>';
    var sl = document.createElement("div"); sl.className = "slot";
    var p = PK.pill({ h: 44, win: { w: 140, h: 36, r: 14, A: W }, right: right });
    if (label) { var l = document.createElement("span"); l.className = "pv-time"; l.textContent = label; l.style.left = Math.max(24, Math.min(118, 4 + (sunX(min) - 17.6) * (140 / 140.8))) + "px"; l.style.top = "24px"; p.appendChild(l); }
    sl.appendChild(p); s.appendChild(sl); d.appendChild(s);
    var zl = document.createElement("div"); zl.className = "zl"; zl.textContent = "true size above · 2x zoom below"; d.appendChild(zl);
    var z = document.createElement("canvas"); z.setAttribute("width", 280); z.setAttribute("height", 72); z.dataset.r = 16; z.className = "zoomc"; z.setAttribute("role", "img"); z.setAttribute("aria-label", cap); d.appendChild(z);
    Y.loop(function (t) { Y.worldWindow(z, W(t), T(t)); });
    var c = document.createElement("div"); c.className = "cap"; c.textContent = cap; d.appendChild(c);
    return d;
  }

  var host = document.getElementById("fcRows");
  ROWS.forEach(function (r) {
    var tr = document.createElement("tr"); tr.dataset.shot = "row-" + r[0];
    var td0 = document.createElement("td"); td0.className = "lab"; td0.innerHTML = "<b>" + r[1] + "</b><p>" + r[2] + "</p>"; tr.appendChild(td0);
    COLS.forEach(function (k, i) { var td = document.createElement("td"); td.appendChild(cell(OPT[k](r[3]), r[4][k], r[6], r[5], r[7][i])); tr.appendChild(td); });
    host.appendChild(tr);
  });
  document.documentElement.dataset.fcReady = "1";
})();
