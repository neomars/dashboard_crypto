"use strict";

const { invoke } = window.__TAURI__.core;
const openUrl = (url) => window.__TAURI__.opener.openUrl(url);

// Équivalent du thème « plotly_dark » de la version Python.
const DARK_TEMPLATE = {
  layout: {
    paper_bgcolor: "#111111",
    plot_bgcolor: "#111111",
    font: { color: "#f2f5fa" },
    colorway: ["#636efa", "#EF553B", "#00cc96", "#ab63fa", "#FFA15A", "#19d3f3", "#FF6692", "#B6E880", "#FF97FF", "#FECB52"],
    xaxis: { gridcolor: "#283442", linecolor: "#506784", zerolinecolor: "#283442", automargin: true },
    yaxis: { gridcolor: "#283442", linecolor: "#506784", zerolinecolor: "#283442", automargin: true },
    polar: { bgcolor: "#111111", angularaxis: { gridcolor: "#506784", linecolor: "#506784" }, radialaxis: { gridcolor: "#506784", linecolor: "#506784" } },
    hoverlabel: { align: "left" },
    title: { x: 0.05 },
    updatemenudefaults: { bgcolor: "#506784", borderwidth: 0 },
  },
};
const PLOT_CONFIG = { responsive: true, displaylogo: false };

const PAIRS = ["BTC", "ETH", "SOL", "DOGE", "AVAX", "LINK"];
const LONG_SHORT_MODES = ["Long vs Short", "Ratio Long/Short", "Open Interest"];
const SOURCES = [
  ["Yahoo Finance", "https://finance.yahoo.com", "prix, volumes et historiques (BTC et autres tickers)"],
  ["BGeometrics", "https://bitcoin-data.com", "métriques on-chain : SOPR, NRPL, HODL waves, ETF (gratuit : 8 requêtes/heure, 15/jour, 4 dernières années ; données gardées 12 h sur le disque)"],
  ["OKX", "https://www.okx.com", "positions long/short et open interest des contrats perpétuels"],
  ["alternative.me", "https://alternative.me/crypto/fear-and-greed-index/", "indice Fear & Greed"],
  ["mempool.space", "https://mempool.space", "hauteur de bloc (estimation du prochain halving)"],
  ["Bitcoin-Dataset (GitHub)", "https://github.com/Yrzxiong/Bitcoin-Dataset", "historique du prix BTC 2010-2018"],
  ["Strategy / SEC", "https://www.strategy.com/purchases", "BTC détenus et actions en circulation de Strategy (MSTR) : fichier intégré à l'application, complétable dans config.ini"],
];

const state = {
  indicators: [],
  selection: "home",
  scale: {},       // échelle choisie par indicateur
  params: {        // paramètres saisis par indicateur
    bmsb: { sma: 20, ema: 21 },
    long_short: { pair: "BTC", mode: "Long vs Short" },
    mstr_mnav: { btc: true },
  },
  sim: null,
  requestId: 0,
};

const main = document.getElementById("main");

// ---------- Utilitaires ----------

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else if (k === "class") node.className = v;
    else if (v === true) node.setAttribute(k, "");
    else node.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c == null || c === false) continue;
    node.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return node;
}

const notice = (level, text) => el("div", { class: `notice ${level}` }, text);
const loading = (text) => el("div", { class: "loading" }, el("div", { class: "spinner" }), text);

function fmtNumber(v, digits = 2) {
  return v.toLocaleString("fr-FR", { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

function today() {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function debounce(fn, ms) {
  let t;
  return (...args) => { clearTimeout(t); t = setTimeout(() => fn(...args), ms); };
}

function plot(container, figure, yScale) {
  const layout = { ...figure.layout, template: DARK_TEMPLATE, autosize: true };
  if (yScale) layout.yaxis = { ...(layout.yaxis || {}), type: yScale };
  const div = el("div", { class: "chart" });
  container.append(div);
  Plotly.newPlot(div, figure.data, layout, PLOT_CONFIG);
  return div;
}

// ---------- Navigation ----------

function renderNav() {
  const nav = document.getElementById("nav");
  nav.replaceChildren();
  const button = (id, label) => el("button", {
    type: "button",
    "aria-current": state.selection === id ? "page" : null,
    onclick: () => select(id),
  }, label);

  nav.append(button("home", "🏠 Accueil"));
  const special = state.indicators.filter((i) => i.is_special);
  if (special.length) {
    nav.append(el("h3", {}, "Simulation"));
    special.forEach((i) => nav.append(button(i.id, `${i.icon} ${i.name}`)));
  }
  nav.append(el("h3", {}, "Indicateurs"));
  state.indicators.filter((i) => !i.is_special).forEach((i) => nav.append(button(i.id, `${i.icon} ${i.name}`)));
}

function renderScale() {
  const box = document.getElementById("scale");
  const ind = current();
  box.hidden = !ind;
  if (!ind) return;
  const value = scaleFor(ind);
  box.querySelectorAll("input").forEach((r) => { r.checked = r.value === value; });
}

function scaleFor(ind) {
  return state.scale[ind.id] || (ind.default_scale === "logarithmique" ? "log" : "linear");
}

const current = () => state.indicators.find((i) => i.id === state.selection);

function select(id) {
  state.selection = id;
  renderNav();
  renderScale();
  render();
  main.scrollTop = 0;
  window.scrollTo(0, 0);
}

function render() {
  const ind = current();
  if (!ind) return renderHome();
  if (ind.id === "simulator") return renderSimulator(ind);
  return renderIndicator(ind);
}

// ---------- Accueil ----------

async function renderHome() {
  main.replaceChildren(
    el("h1", {}, "📊 Tableau de Bord d'Indicateurs Financiers"),
    el("p", {}, "Cette application permet de visualiser différents indicateurs sur les marchés crypto et financiers."),
  );

  const config = el("div", { class: "card" }, loading("Chargement de la configuration…"));
  main.append(el("h2", {}, "🌐 Sources et configuration"), config);

  main.append(el("h2", {}, "Explorez nos outils"));
  const tools = el("div", { class: "tools" });
  state.indicators.forEach((i) => tools.append(el("button", { class: "tool", type: "button", onclick: () => select(i.id) },
    el("strong", {}, `${i.icon} ${i.name}`), el("span", {}, i.description))));
  main.append(tools);

  renderSettings(config);
}

async function renderSettings(box) {
  let settings;
  try {
    settings = await invoke("get_settings");
  } catch (e) {
    box.replaceChildren(notice("error", String(e)));
    return;
  }
  const status = el("div");
  const show = (level, text) => status.replaceChildren(notice(level, text));
  const link = (label, url) => el("a", { href: "#", onclick: (e) => { e.preventDefault(); openUrl(url); } }, label);

  const rows = settings.endpoints.map((m) => {
    const input = el("input", { type: "text", value: m.value, placeholder: m.candidates.join(" / "), spellcheck: "false" });
    const save = el("button", { class: "btn", type: "button", onclick: async () => {
      try {
        await invoke("save_endpoint", { key: m.key, value: input.value });
        show("success", input.value.trim() ? `Endpoint enregistré pour ${m.label}.` : `${m.label} : détection automatique rétablie.`);
      } catch (e) { show("error", String(e)); }
    } }, "Enregistrer");
    return el("tr", {}, el("td", {}, m.label), el("td", {}, input), el("td", {}, save));
  });

  box.replaceChildren(
    el("h3", {}, "Sources de données"),
    el("p", {}, "Toutes les données viennent d'API publiques et gratuites, sans clé ni compte :"),
    el("ul", {}, SOURCES.map(([name, url, what]) => el("li", {}, link(name, url), ` : ${what}`))),
    el("h3", {}, "Endpoints BGeometrics"),
    el("p", { class: "muted" }, "Laissez vide pour la détection automatique (les noms indiqués en gris sont essayés dans l'ordre). Si un indicateur ne trouve pas ses données, indiquez ici le nom exact de l'endpoint, visible dans la ",
      link("documentation de l'API", "https://bitcoin-data.com/api/redoc.html"), "."),
    el("table", { class: "queries" }, el("thead", {}, el("tr", {}, el("th", {}, "Métrique"), el("th", {}, "Endpoint"), el("th"))), el("tbody", {}, rows)),
    status,
    el("h3", {}, "MSTR mNAV : bitcoins détenus"),
    settings.mstr
      ? el("p", {}, `Dernière donnée utilisée : ${settings.mstr.date}, ${fmtNumber(settings.mstr.btc, 0)} BTC détenus, ${fmtNumber(settings.mstr.shares, 0)} actions.`)
      : el("p", {}, "Aucune donnée de holdings MSTR."),
    el("p", { class: "muted" }, "Pour ajouter les achats annoncés depuis (", link("strategy.com/purchases", "https://www.strategy.com/purchases"),
      "), ajoutez des lignes « AAAA-MM-JJ = BTC détenus » ou « AAAA-MM-JJ = BTC détenus, actions en circulation » dans la section ",
      el("code", {}, "[MSTR]"), " du fichier de configuration ci-dessous. Les seuils de couleur se règlent avec ", el("code", {}, "seuils = 1.0, 1.5, 2.5"), "."),
    el("p", { class: "muted" }, "Fichier de configuration : ", el("code", {}, settings.config_path)),
  );
}

// ---------- Indicateurs ----------

function indicatorControls(ind, refresh) {
  const params = state.params[ind.id];
  if (ind.id === "bmsb") {
    const slider = (key, label) => {
      const out = el("output", {}, params[key]);
      const input = el("input", { type: "range", min: 10, max: 50, step: 1, value: params[key],
        oninput: (e) => { out.textContent = e.target.value; },
        onchange: (e) => { params[key] = Number(e.target.value); refresh(); } });
      return el("label", { class: "field" }, el("span", {}, label, " : ", out), input);
    };
    return el("div", { class: "controls" }, slider("sma", "Longueur SMA (Semaines)"), slider("ema", "Longueur EMA (Semaines)"));
  }
  if (ind.id === "long_short") {
    const pair = el("select", { onchange: (e) => { params.pair = e.target.value; refresh(); } },
      PAIRS.map((p) => el("option", { value: p, selected: p === params.pair }, p)));
    const modes = el("div", { class: "radios" }, LONG_SHORT_MODES.map((m) => el("label", {},
      el("input", { type: "radio", name: "ls-mode", value: m, checked: m === params.mode,
        onchange: () => { params.mode = m; refresh(); } }), " ", m)));
    return el("div", { class: "controls" }, el("label", { class: "field" }, el("span", {}, "Paire"), pair),
      el("div", { class: "field" }, el("span", {}, "Mode d'affichage"), modes));
  }
  if (ind.id === "mstr_mnav") {
    const toggle = el("input", { type: "checkbox", checked: params.btc,
      onchange: (e) => { params.btc = e.target.checked; refresh(); } });
    return el("div", { class: "controls" }, el("label", {}, toggle, " Afficher le prix du BTC (axe de droite)"));
  }
  return null;
}

function renderIndicator(ind) {
  const results = el("div");
  const refresh = () => loadIndicator(ind, results);
  main.replaceChildren(el("h1", {}, `${ind.icon} ${ind.name}`), el("p", { class: "muted" }, ind.description));
  const controls = indicatorControls(ind, refresh);
  if (controls) main.append(controls);
  main.append(results);
  refresh();
}

async function loadIndicator(ind, box) {
  const id = ++state.requestId;
  box.replaceChildren(loading(`Chargement de ${ind.name}…`));
  let out;
  try {
    out = await invoke("render_indicator", { id: ind.id, params: state.params[ind.id] || null });
  } catch (e) {
    if (id === state.requestId) box.replaceChildren(notice("error", `Erreur lors du chargement de l'indicateur : ${e}`));
    return;
  }
  if (id !== state.requestId) return; // une autre page a été demandée entre-temps

  box.replaceChildren();
  if (out.metrics.length) {
    box.append(el("div", { class: "metrics" }, out.metrics.map((m) =>
      el("div", { class: "metric" }, el("div", { class: "label" }, m.label), el("div", { class: "value" }, m.value)))));
  }
  out.notices.forEach((n) => box.append(notice(n.level, n.text)));
  plot(box, out.figure, scaleFor(ind));
  if (ind.interpretation) box.append(notice("info", `💡 Interprétation : ${ind.interpretation}`));
}

// ---------- Simulateur ----------

function renderSimulator(ind) {
  const p = state.sim || (state.sim = {
    start: "2017-01-01", end: today(), initial_capital: 10000, target_leverage: 2.0, ticker: "BTC-USD",
    drop_pct: 10.0, exit_frequency: "Hebdomadaire", exit_pct: 10.0,
  });
  const results = el("div");
  const run = debounce(() => runSimulation(ind, results), 400);
  const field = (label, input) => el("label", { class: "field" }, el("span", {}, label), input);
  const bind = (key, type, attrs = {}) => el("input", { type, value: p[key], ...attrs,
    onchange: (e) => { p[key] = type === "number" || type === "range" ? Number(e.target.value) : e.target.value; run(); } });

  const dropOut = el("output", {}, p.drop_pct);
  const drop = el("input", { type: "range", min: 1, max: 50, step: 0.5, value: p.drop_pct,
    oninput: (e) => { dropOut.textContent = e.target.value; },
    onchange: (e) => { p.drop_pct = Number(e.target.value); run(); } });
  const freq = el("select", { onchange: (e) => { p.exit_frequency = e.target.value; run(); } },
    ["Journalière", "Hebdomadaire", "Mensuelle"].map((f) => el("option", { value: f, selected: f === p.exit_frequency }, f)));

  main.replaceChildren(
    el("h1", {}, `${ind.icon} ${ind.name} `, el("small", {}, "(BTC est la valeur par défaut, aucune position en stablecoin)")),
    el("p", { class: "muted" }, ind.description),
    el("div", { class: "controls" },
      field("Date de début", bind("start", "date")),
      field("Date de fin", bind("end", "date")),
      field("Investissement initial (USD)", bind("initial_capital", "number", { step: 100, min: 1 })),
      field("Effet de levier cible", bind("target_leverage", "number", { step: 0.1, min: 1 })),
      field("Ticker Yahoo Finance", bind("ticker", "text", { title: "Exemples : BTC-USD, ETH-USD, SOL-USD, AAPL, GC=F" }))),
    el("div", { class: "controls" },
      el("label", { class: "field" }, el("span", {}, "Baisse déclencheur (%) : ", dropOut), drop),
      field("Fréquence de sortie", freq),
      field("Sortie par étape (%)", bind("exit_pct", "number", { step: 1, min: 1, max: 100 }))),
    el("p", { class: "muted" }, "Exemples de tickers : BTC-USD, ETH-USD, SOL-USD, AAPL, GC=F"),
    results,
  );
  runSimulation(ind, results);
}

async function runSimulation(ind, box) {
  const id = ++state.requestId;
  const p = state.sim;
  if (p.start >= p.end) {
    box.replaceChildren(notice("error", "La date de début doit être antérieure à la date de fin."));
    return;
  }
  box.replaceChildren(loading("Simulation en cours…"));
  let res;
  try {
    res = await invoke("run_simulation", { params: p });
  } catch (e) {
    if (id === state.requestId) box.replaceChildren(notice("error", String(e)));
    return;
  }
  if (id !== state.requestId) return;

  const s = res.summary;
  const unit = p.ticker.split("-")[0];
  const metric = (label, value, delta, sub, inverse) => el("div", { class: "metric" },
    el("div", { class: "label" }, label), el("div", { class: "value" }, value),
    delta != null && el("div", { class: `delta ${(delta >= 0) !== !!inverse ? "up" : "down"}` }, `${delta >= 0 ? "▲" : "▼"} ${fmtNumber(delta)} %`),
    sub && el("div", { class: "sub" }, sub));

  box.replaceChildren();
  plot(box, res.figure, scaleFor(ind));
  box.append(
    el("h2", {}, "📈 Résultat de la stratégie"),
    el("div", { class: "metrics" },
      metric("Capital final (USD)", `${fmtNumber(s.final_equity)} $`, s.performance_pct, `Initial : ${fmtNumber(p.initial_capital)} $`),
      metric(`Capital final (${unit})`, `${fmtNumber(s.final_units, 4)} ${unit}`, null, `Initial : ${fmtNumber(s.initial_units, 4)} ${unit}`),
      metric("Drawdown max", `${fmtNumber(s.max_drawdown)} %`),
      metric("Buy & Hold", `${fmtNumber(s.buy_hold)} $`, s.buy_hold_pct)),
  );
  if (s.liquidated) box.append(notice("error", "💀 ALERTE : votre stratégie a été liquidée ! Le capital est tombé à zéro suite aux pertes sous levier."));

  if (res.trades.length) {
    box.append(el("h2", {}, "📝 Journal des opérations"), el("div", { class: "table-wrap" }, el("table", {},
      el("thead", {}, el("tr", {}, ["Date", "Action", "Prix", "Détails"].map((h) => el("th", {}, h)))),
      el("tbody", {}, res.trades.map((t) => el("tr", {}, el("td", {}, t.date), el("td", {}, t.action), el("td", {}, t.price), el("td", {}, t.details)))))));
  }

  const status = el("div");
  const exportAs = async (format) => {
    try {
      const path = await invoke("export_simulation", { format });
      if (path) status.replaceChildren(notice("success", `Fichier enregistré : ${path}`));
    } catch (e) { status.replaceChildren(notice("error", String(e))); }
  };
  box.append(el("h2", {}, "📥 Exporter les résultats"), el("div", { class: "row" },
    el("button", { class: "btn", type: "button", onclick: () => exportAs("csv") }, "Télécharger CSV (Historique)"),
    el("button", { class: "btn", type: "button", onclick: () => exportAs("pdf") }, "Télécharger le rapport PDF")), status);
}

// ---------- Démarrage ----------

document.querySelectorAll("#scale input").forEach((radio) => radio.addEventListener("change", () => {
  const ind = current();
  if (!ind) return;
  state.scale[ind.id] = radio.value;
  main.querySelectorAll(".chart").forEach((div) => Plotly.relayout(div, { "yaxis.type": radio.value }));
}));

(async () => {
  try {
    state.indicators = await invoke("list_indicators");
    document.getElementById("version").textContent = `Version ${await invoke("app_version")}`;
  } catch (e) {
    main.replaceChildren(notice("error", `Impossible de démarrer : ${e}`));
    return;
  }
  renderNav();
  renderScale();
  render();
})();
