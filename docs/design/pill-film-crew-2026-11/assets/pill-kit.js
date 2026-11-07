/* Proposed pill shell + shared extras (headphones, sweat, zzz, lock) used by the
 * pill and companions pages. Pure DOM; Yappy art comes from yappy.js. */
(function () {
  "use strict";
  var Y = window.Yappy;
  var IC = {
    mic: '<path d="M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3z"/><path d="M5 11a7 7 0 0 0 14 0"/><path d="M12 18v3"/>',
    rec: '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3" fill="currentColor"/>',
    note: '<path d="M15 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M15 3v5h5"/><path d="M8 13h8M8 17h5"/>',
    stop: '<rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" stroke="none"/>',
    lock: '<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>',
    check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
    retry: '<path d="M4 12a8 8 0 1 0 2.3-5.6"/><path d="M4 4v4h4"/>',
    batt: '<rect x="3" y="8" width="16" height="9" rx="2"/><path d="M21 11v3"/><path d="M6 11v3"/>',
    paste: '<rect x="8" y="3" width="8" height="4" rx="1"/><path d="M16 5h2a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2"/>',
    cal: '<rect x="4" y="5" width="16" height="15" rx="2"/><path d="M4 10h16M9 3v4M15 3v4"/>',
    x: '<path d="M6 6l12 12M18 6L6 18"/>',
    expand: '<path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7"/>',
    copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>',
    mute: '<path d="M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3z"/><path d="M5 11a7 7 0 0 0 14 0"/><path d="M3 3l18 18"/>',
    spark: '<path d="M12 4v4M12 16v4M4 12h4M16 12h4"/>'
  };
  function icon(n, s) { return '<svg class="ic" width="' + (s || 16) + '" height="' + (s || 16) + '" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + IC[n] + '</svg>'; }
  var X = {
    headphones: function (o, cx, cy, rx, ry, h) { h.rct(cx - rx + 1, cy - ry - 1, 2 * rx - 2, 2, "#3b3f4a"); h.rct(cx - rx - 2, cy - ry + 1, 2, 6, "#3b3f4a"); h.rct(cx + rx, cy - ry + 1, 2, 6, "#3b3f4a"); h.rct(cx - rx - 3, cy - 3, 4, 6, "#e0574a"); h.rct(cx + rx - 1, cy - 3, 4, 6, "#e0574a"); },
    sweat: function (o, cx, cy, rx, ry, h) { h.rct(cx + rx - 1, cy - 6, 2, 2, "#8fd6ff"); h.rct(cx + rx - 1, cy - 4, 2, 2, "#5fb8f0"); },
    zzz: function (o, cx, cy, rx, ry, h) { var c = "#5a6280"; h.rct(cx + rx, cy - ry - 2, 3, 1, c); h.rct(cx + rx + 1, cy - ry - 1, 1, 1, c); h.rct(cx + rx, cy - ry, 3, 1, c); h.rct(cx + rx + 4, cy - ry - 6, 2, 1, c); h.rct(cx + rx + 4, cy - ry - 5, 2, 1, c); },
    question: function (o, cx, cy, rx, ry, h) { var c = "#3a2f36"; h.rct(cx + rx + 1, cy - ry, 3, 1, c); h.rct(cx + rx + 3, cy - ry + 1, 1, 2, c); h.rct(cx + rx + 2, cy - ry + 3, 1, 1, c); h.rct(cx + rx + 2, cy - ry + 5, 1, 1, c); },
    lock: function (o, cx, cy, rx, ry, h) { h.rct(cx + rx, cy - 2, 6, 5, "#b7791f"); h.rct(cx + rx + 1, cy - 5, 1, 3, "#7a5212"); h.rct(cx + rx + 4, cy - 5, 1, 3, "#7a5212"); h.rct(cx + rx + 1, cy - 5, 4, 1, "#7a5212"); },
    leaf: function (o, cx, cy, rx, ry, h) { h.rct(cx + rx + 1, cy - 2, 5, 3, "#7fbf6a"); h.rct(cx + rx + 2, cy - 1, 3, 1, "#5f9f4e"); },
    shrug: function (o, cx, cy, rx, ry, h) { h.rct(cx - rx - 2, cy - 2, 2, 1, "#6b3a2e"); h.rct(cx + rx + 1, cy - 2, 2, 1, "#6b3a2e"); }
  };
  function chain() { var fs = [].slice.call(arguments).filter(Boolean); return function () { var a = arguments; fs.forEach(function (f) { f.apply(null, a); }); }; }
  /* cfg: {w,h,win:{w,h,A(t)}, left:'html', right:'html', dock, cls, bubble:{text, side}} */
  function pill(cfg) {
    var el = document.createElement("div");
    el.className = "ypill " + (cfg.cls || "");
    el.style.height = cfg.h + "px"; if (cfg.w) el.style.width = cfg.w + "px";
    el.style.borderRadius = (cfg.radius != null ? cfg.radius : cfg.h / 2) + "px";
    var inner = "";
    if (cfg.left) inner += '<div class="seg l">' + cfg.left + '</div>';
    if (cfg.win) { var r = cfg.win.r != null ? cfg.win.r : Math.max(4, cfg.h / 2 - 4); inner += '<canvas class="win" width="' + cfg.win.w + '" height="' + cfg.win.h + '" data-r="' + r + '"></canvas>'; }
    if (cfg.right) inner += '<div class="seg r">' + cfg.right + '</div>';
    el.innerHTML = inner;
    if (cfg.bubble) { var b = document.createElement("div"); b.className = "ybub " + (cfg.bubble.side || "below"); b.textContent = cfg.bubble.text; el.appendChild(b); }
    if (cfg.win) { var cv = el.querySelector("canvas"); var draw = function (t) { Y.worldWindow(cv, cfg.win.A(t), t); }; Y.loop(draw); }
    return el;
  }
  window.PK = { icon: icon, X: X, chain: chain, pill: pill };
})();
