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
// Indicateur « Top Traders » : monnaies proposées (contrats perpétuels USDT de Binance) ; toute
// autre monnaie se saisit via « Autre… ». Périodes : intervalles de l'API Binance.
const TOP_TRADER_COINS = ["BTC", "ETH", "BNB", "SOL", "XRP", "DOGE", "ADA", "TRX", "AVAX", "LINK", "LTC"];
const TOP_TRADER_PERIODS = [["5m", "5 minutes"], ["15m", "15 minutes"], ["30m", "30 minutes"], ["1h", "1 heure"],
  ["2h", "2 heures"], ["4h", "4 heures"], ["6h", "6 heures"], ["12h", "12 heures"], ["1d", "1 jour"]];
const SOURCES = [
  ["Yahoo Finance", "https://finance.yahoo.com", "prix, volumes et historiques (BTC et autres tickers)"],
  ["BGeometrics", "https://bitcoin-data.com", "métriques on-chain : SOPR, NRPL, HODL waves, ETF (gratuit : 8 requêtes/heure, 15/jour, 4 dernières années ; données gardées 12 h sur le disque)"],
  ["OKX", "https://www.okx.com", "positions long/short et open interest des contrats perpétuels"],
  ["Binance", "https://www.binance.com", "cours (bougies) et ratio long/short des meilleurs traders (comptes) des contrats perpétuels USDT ; le ratio ne couvre que les 30 derniers jours"],
  ["alternative.me", "https://alternative.me/crypto/fear-and-greed-index/", "indice Fear & Greed"],
  ["mempool.space", "https://mempool.space", "hauteur de bloc (estimation du prochain halving)"],
  ["Bitcoin-Dataset (GitHub)", "https://github.com/Yrzxiong/Bitcoin-Dataset", "historique du prix BTC 2010-2018"],
  ["ISM PMI (ism-feed, GitHub)", "https://github.com/sofiaverma06/ism-feed", "ISM Manufacturing PMI mensuel : fichier intégré (2013 →), complété par ce CSV public et par config.ini (section [PMI])"],
  ["Strategy / SEC EDGAR", "https://www.sec.gov/edgar/browse/?CIK=1050446", "BTC détenus et actions en circulation de Strategy (MSTR) : fichier intégré, complété à chaque affichage par les dernières publications SEC (contact requis, voir plus bas) et par config.ini"],
  ["Poloniex", "https://api-docs.poloniex.com", "section Trading uniquement : bougies spot et perpétuel, historique du funding (180 derniers jours seulement) ; page Fibonacci à levier : bougies 1 minute (gardées sur disque), fiche du contrat, prix de marque si disponible"],
  ["Coin Metrics (GitHub)", "https://github.com/coinmetrics/data", "section Trading, Accumulation BTC : historique du MVRV du bitcoin (fichier CSV public, mis à jour irrégulièrement)"],
];

// Section Trading : actifs disponibles en spot et en perpétuel USDT sur Poloniex.
const TRADING_ASSETS = ["BTC", "ETH", "SOL", "XRP", "DOGE", "LTC", "BNB", "ADA", "AVAX", "LINK", "TRX"];
const MARKETS = [["spot", "Spot"], ["perp", "Perpétuel USDT"]];
const INTERVALS = [["DAY_1", "1 jour"], ["HOUR_4", "4 heures"]];
const VARIANTS = [
  ["donchian", "Ensemble Donchian (20, 55, 100, 200)"],
  ["ema", "Croisement EMA 20/100 + filtre SMA 200"],
  ["breakout", "Cassure 55 j, sortie sous moyenne 200 j"],
];
const WEIGHTINGS = [
  ["fixed", "Fixe (référence)"],
  ["mvrv_tiers", "MVRV par paliers"],
  ["mvrv_inverse", "MVRV inverse (1 / MVRV)"],
  ["price_ma200", "Prix / moyenne 200 jours"],
];
const WEEKDAYS = [[1, "Lundi"], [2, "Mardi"], [3, "Mercredi"], [4, "Jeudi"], [5, "Vendredi"], [6, "Samedi"], [7, "Dimanche"]];

const state = {
  indicators: [],
  trading: [],     // pages du moteur de trading, séparées des indicateurs de marché
  selection: "home",
  scale: {},       // échelle choisie par indicateur
  params: {        // paramètres saisis par indicateur
    bmsb: { sma: 20, ema: 21 },
    long_short: { pair: "BTC", mode: "Long vs Short" },
    top_traders: { coin: "BTC", period: "4h" },
    mstr_mnav: { btc: true },
    pmi_btc: { sma: 24 },
  },
  sim: null,       // réglages du simulateur, mémorisés dans config.ini
  simSaved: null,  // dernière version enregistrée (JSON), pour n'écrire qu'après un changement
  signal: { base: "BTC", market: "spot", interval: "DAY_1", variant: "donchian" },
  bt: null,        // paramètres du backtest de la section Trading
  fibo: null,      // paramètres de la page Fibonacci à levier
  acc: null,       // paramètres de l'accumulation BTC
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

// `axes` : axes verticaux qui suivent le choix d'échelle (le premier par défaut).
function plot(container, figure, yScale, axes = ["yaxis"]) {
  const layout = { ...figure.layout, template: DARK_TEMPLATE, autosize: true };
  if (yScale) for (const a of axes) layout[a] = { ...(layout[a] || {}), type: yScale };
  const div = el("div", { class: "chart", "data-axes": axes.join(",") });
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
  if (state.trading.length) {
    nav.append(el("h3", {}, "Trading"));
    state.trading.forEach((t) => nav.append(button(t.id, `${t.icon} ${t.name}`)));
  }
  const special = state.indicators.filter((i) => i.is_special);
  if (special.length) {
    nav.append(el("h3", {}, "Simulation"));
    special.forEach((i) => nav.append(button(i.id, `${i.icon} ${i.name}`)));
  }
  nav.append(el("h3", {}, "Indicateurs de marché"));
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

const current = () => [...state.trading, ...state.indicators].find((i) => i.id === state.selection);

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
  if (ind.id === "btc_accumulation") return renderAccumulation(ind);
  if (ind.id === "fibo_leverage") return renderFibo(ind);
  if (ind.id === "trend_signals") return renderTrendSignals(ind);
  if (ind.id === "trend_backtest") return renderBacktest(ind);
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
  const tools = (items) => el("div", { class: "tools" }, items.map((i) => el("button", { class: "tool", type: "button", onclick: () => select(i.id) },
    el("strong", {}, `${i.icon} ${i.name}`), el("span", {}, i.description))));
  if (state.trading.length) {
    main.append(el("h3", {}, "Trading"),
      el("p", { class: "muted" }, "Moteur de trading séparé : ses propres données (Poloniex, MVRV), ses indicateurs de signal, ses règles de risque, son backtest et son accumulation de BTC. L'objectif se mesure en nombre de BTC, pas en dollars. Aucun ordre réel n'est passé."),
      tools(state.trading));
  }
  main.append(el("h3", {}, "Indicateurs de marché et simulation"),
    el("p", { class: "muted" }, "Outils d'analyse : ils n'alimentent pas le moteur de trading et ne déclenchent aucun ordre."),
    tools(state.indicators));

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
    el("h3", {}, "Contact SEC"),
    el("p", { class: "muted" }, "L'indicateur MSTR mNAV lit les dernières publications de Strategy à la SEC (EDGAR). La SEC exige que chaque requête indique une adresse e-mail de contact ; sans elle, elle répond « HTTP 403 ». Cette adresse n'est envoyée qu'à la SEC."),
    secContactField(settings.sec_user_agent),
    el("p", { class: "muted" }, "Fichier de configuration : ", el("code", {}, settings.config_path)),
  );
}

// Champ « e-mail de contact SEC » (page Accueil et indicateur MSTR mNAV).
// `onSaved` est appelé après un enregistrement réussi (ex. relancer l'indicateur).
function secContactField(value, onSaved) {
  const status = el("div");
  const input = el("input", { type: "email", value: value || "", size: 36, spellcheck: "false",
    placeholder: "votre.adresse@exemple.fr", "aria-label": "E-mail de contact SEC" });
  const save = async () => {
    try {
      await invoke("save_sec_user_agent", { value: input.value });
      status.replaceChildren(notice("success", input.value.trim()
        ? "E-mail de contact SEC enregistré."
        : "E-mail de contact SEC retiré."));
      if (onSaved) onSaved();
    } catch (e) { status.replaceChildren(notice("error", String(e))); }
  };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") save(); });
  return el("div", {},
    el("div", { class: "row" }, el("span", {}, "E-mail de contact SEC :"), input,
      el("button", { class: "btn", type: "button", onclick: save }, "Enregistrer")),
    status);
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
  if (ind.id === "top_traders") {
    // « Autre… » affiche un champ où saisir n'importe quel contrat perpétuel USDT de Binance (ARB, 1000PEPE...).
    const OTHER = "other";
    const listed = TOP_TRADER_COINS.includes(params.coin);
    const custom = el("input", { type: "text", value: listed ? "" : params.coin, size: 12, spellcheck: "false",
      placeholder: "ex. ARB, 1000PEPE", "aria-label": "Autre monnaie", hidden: listed,
      onchange: (e) => {
        const coin = e.target.value.trim().toUpperCase();
        e.target.value = coin;
        if (coin) { params.coin = coin; refresh(); }
      } });
    const coin = el("select", { "aria-label": "Monnaie", onchange: (e) => {
        const other = e.target.value === OTHER;
        custom.hidden = !other;
        if (other) custom.focus();
        else { params.coin = e.target.value; refresh(); }
      } },
      TOP_TRADER_COINS.map((c) => el("option", { value: c, selected: c === params.coin }, c)),
      el("option", { value: OTHER, selected: !listed }, "Autre…"));
    const period = el("select", { "aria-label": "Période", onchange: (e) => { params.period = e.target.value; refresh(); } },
      TOP_TRADER_PERIODS.map(([code, label]) => el("option", { value: code, selected: code === params.period }, label)));
    return el("div", { class: "controls" },
      el("div", { class: "field" }, el("span", {}, "Monnaie"), el("div", { class: "row" }, coin, custom)),
      el("label", { class: "field" }, el("span", {}, "Période"), period));
  }
  if (ind.id === "pmi_btc") {
    const out = el("output", {}, params.sma);
    const input = el("input", { type: "range", min: 6, max: 60, step: 1, value: params.sma,
      oninput: (e) => { out.textContent = e.target.value; },
      onchange: (e) => { params.sma = Number(e.target.value); refresh(); } });
    return el("div", { class: "controls" }, el("label", { class: "field" }, el("span", {}, "Moyenne de tendance BTC (mois) : ", out), input));
  }
  if (ind.id === "mstr_mnav") {
    const toggle = el("input", { type: "checkbox", checked: params.btc,
      onchange: (e) => { params.btc = e.target.checked; refresh(); } });
    // Pré-rempli avec l'adresse enregistrée ; un enregistrement relance la récupération SEC.
    const contact = el("div");
    invoke("get_settings")
      .then((s) => contact.replaceChildren(secContactField(s.sec_user_agent, refresh)))
      .catch(() => contact.replaceChildren(secContactField("", refresh)));
    return el("div", {},
      el("div", { class: "controls" }, el("label", {}, toggle, " Afficher le prix du BTC (axe de droite)")),
      el("p", { class: "muted" }, "Les données récentes de Strategy viennent de la SEC, qui exige une adresse e-mail de contact (envoyée uniquement à la SEC)."),
      contact);
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

// Nom de la valeur simulée, tiré du ticker (même règle que SimParams::unit) : ETH-USD → ETH.
const unitOf = (ticker) => {
  const t = ticker.trim();
  return (t.split("-")[0] || t).toUpperCase();
};

async function renderSimulator(ind) {
  // Réglages du dernier lancement (ou ceux par défaut), lus une seule fois par session.
  if (!state.sim) {
    main.replaceChildren(loading("Chargement des réglages…"));
    try {
      state.sim = await invoke("get_simulator_params");
    } catch (e) {
      main.replaceChildren(notice("error", `Impossible de lire les réglages du simulateur : ${e}`));
      return;
    }
    state.simSaved = JSON.stringify(state.sim);
    if (state.selection !== ind.id) return; // une autre page a été demandée entre-temps
  }
  const p = state.sim;
  const results = el("div");
  const run = debounce(() => runSimulation(ind, results), 400);
  const field = (label, input) => el("label", { class: "field" }, el("span", {}, label), input);
  const bind = (key, type, attrs = {}) => el("input", { type, value: p[key], ...attrs,
    onchange: (e) => { p[key] = type === "number" || type === "range" ? Number(e.target.value) : e.target.value; run(); } });
  // Sous-titre mis à jour pendant la saisie du ticker.
  const subtitle = el("small", {});
  const setSubtitle = (ticker) => {
    const unit = unitOf(ticker);
    subtitle.textContent = unit ? `(valeur simulée : ${unit}, aucune position en stablecoin)` : "(aucune position en stablecoin)";
  };
  setSubtitle(p.ticker);

  const dropOut = el("output", {}, p.drop_pct);
  const drop = el("input", { type: "range", min: 1, max: 50, step: 0.5, value: p.drop_pct,
    oninput: (e) => { dropOut.textContent = e.target.value; },
    onchange: (e) => { p.drop_pct = Number(e.target.value); run(); } });
  const freq = el("select", { onchange: (e) => { p.exit_frequency = e.target.value; run(); } },
    ["Journalière", "Hebdomadaire", "Mensuelle"].map((f) => el("option", { value: f, selected: f === p.exit_frequency }, f)));

  main.replaceChildren(
    el("h1", {}, `${ind.icon} ${ind.name} `, subtitle),
    el("p", { class: "muted" }, ind.description),
    el("div", { class: "controls" },
      field("Date de début", bind("start", "date")),
      field("Date de fin", bind("end", "date")),
      field("Investissement initial (USD)", bind("initial_capital", "number", { step: 100, min: 1 })),
      field("Effet de levier cible", bind("target_leverage", "number", { step: 0.1, min: 1 })),
      field("Ticker Yahoo Finance", bind("ticker", "text", { title: "Exemples : BTC-USD, ETH-USD, SOL-USD, AAPL, GC=F",
        oninput: (e) => setSubtitle(e.target.value) }))),
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
  const unit = res.unit;
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
      metric(`Buy & Hold ${unit}`, `${fmtNumber(s.buy_hold)} $`, s.buy_hold_pct)),
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

  // Seuls les réglages qui ont donné une simulation sont retenus pour le prochain lancement
  // (un ticker mal saisi, par exemple, ne l'est pas).
  const snapshot = JSON.stringify(p);
  if (snapshot === state.simSaved) return;
  try {
    await invoke("save_simulator_params", { params: p });
    state.simSaved = snapshot;
  } catch (e) {
    if (id === state.requestId) box.append(notice("warning", `Réglages non enregistrés : ${e}`));
  }
}

// ---------- Trading (moteur séparé des indicateurs de marché) ----------

// Prix et quantités : 6 chiffres significatifs (le DOGE cote 0,1 USDT, le BTC 80 000).
const fmtSig = (v) => v.toLocaleString("fr-FR", { maximumSignificantDigits: 6 });
const fmtPct = (v, digits = 1) => (v == null ? "–" : `${fmtNumber(v, digits)} %`);
const fmtRatio = (v) => (v == null ? "–" : fmtNumber(v, 2));
const fmtExposure = (x) => `${x > 0 ? "+" : ""}${fmtNumber(x * 100, 0)} % du capital`;

// Nombre de BTC : 6 décimales sous 1 BTC, 4 au-delà.
const fmtBtc = (v) => (v == null ? "–" : fmtNumber(v, Math.abs(v) < 1 ? 6 : 4));
const fmtSignedBtc = (v) => (v == null ? "–" : `${v > 0 ? "+" : v < 0 ? "−" : ""}${fmtBtc(Math.abs(v))}`);
const fmtSignedPct = (v, digits = 1) => (v == null ? "–" : `${v > 0 ? "+" : v < 0 ? "−" : ""}${fmtNumber(Math.abs(v), digits)} %`);

function metricCard(label, value, sub, delta, primary) {
  return el("div", { class: primary ? "metric primary" : "metric" },
    el("div", { class: "label" }, label), el("div", { class: "value" }, value),
    delta != null && el("div", { class: `delta ${delta >= 0 ? "up" : "down"}` }, `${delta >= 0 ? "▲" : "▼"} ${fmtNumber(delta)} %`),
    sub && el("div", { class: "sub" }, sub));
}

function scoreLabel(score) {
  if (score == null) return "–";
  const side = score > 0 ? "acheteur" : score < 0 ? "vendeur" : "neutre";
  return `${score > 0 ? "+" : ""}${fmtNumber(score, 2)} (${side})`;
}

// Actif, marché, bougies et variante, communs aux deux pages Trading.
function tradingControls(p, onChange) {
  const select = (key, options) => el("select", { onchange: (e) => { p[key] = e.target.value; onChange(key); } },
    options.map(([value, label]) => el("option", { value, selected: value === p[key] }, label)));
  const field = (label, input) => el("label", { class: "field" }, el("span", {}, label), input);
  return el("div", { class: "controls" },
    field("Actif", select("base", TRADING_ASSETS.map((a) => [a, `${a}/USDT`]))),
    field("Marché", select("market", MARKETS)),
    field("Bougies", select("interval", INTERVALS)),
    field("Variante du module tendance", select("variant", VARIANTS)));
}

function renderTrendSignals(ind) {
  const results = el("div");
  const refresh = () => loadTrendSignals(ind, results);
  main.replaceChildren(
    el("h1", {}, `${ind.icon} ${ind.name}`),
    el("p", { class: "muted" }, ind.description),
    tradingControls(state.signal, refresh),
    results,
  );
  refresh();
}

async function loadTrendSignals(ind, box) {
  const id = ++state.requestId;
  box.replaceChildren(loading("Chargement des bougies Poloniex…"));
  let r;
  try {
    r = await invoke("trading_signals", { params: state.signal });
  } catch (e) {
    if (id === state.requestId) box.replaceChildren(notice("error", String(e)));
    return;
  }
  if (id !== state.requestId) return;

  box.replaceChildren(el("div", { class: "metrics" },
    metricCard("Score de tendance", scoreLabel(r.score), `Clôture du ${r.last_date} : ${fmtSig(r.close)} USDT`),
    metricCard("Exposition cible", fmtExposure(r.target_exposure), `Plafond : ${fmtNumber(r.max_leverage, 0)}x, volatilité visée 25 %/an`),
    metricCard("Volatilité réalisée (30 j)", fmtPct(r.realized_vol_pct), "Annualisée"),
    metricCard("Stop indicatif (3 ATR)", r.stop_hint == null ? "–" : `${fmtSig(r.stop_hint)} USDT`, r.atr == null ? null : `ATR 20 : ${fmtSig(r.atr)} USDT`)));
  if (r.components.length) {
    const chip = (c) => {
      const [cls, mark] = c.state > 0 ? ["up", "▲ acheteur"] : c.state < 0 ? ["down", "▼ vendeur"] : ["flat", "– neutre"];
      return el("span", { class: `chip ${cls}` }, `${c.lookback} bougies : ${c.state == null ? "–" : mark}`);
    };
    box.append(el("div", { class: "chips" }, el("span", { class: "muted" }, "Cassures de Donchian :"), r.components.map(chip)));
  }
  plot(box, r.chart, scaleFor(ind));
  if (ind.interpretation) box.append(notice("info", `💡 Interprétation : ${ind.interpretation}`));
}

function renderBacktest(ind) {
  const p = state.bt || (state.bt = {
    // Réglages validés en bougies journalières, au comptant, sans levier ni stop.
    base: "BTC", market: "spot", interval: "DAY_1", variant: "breakout",
    start: "2018-01-01", end: today(), initial_capital: 10000,
    target_vol_pct: 100, max_leverage: 1, stop_atr_mult: 0,
    fee_pct: 0.075, slippage_pct: 0.05, rebalance_pct: 20, funding_pct_8h: 0.01,
    halt_on_breach: false,
  });
  const results = el("div");
  const run = debounce(() => runBacktest(ind, results), 400);
  const field = (label, input) => el("label", { class: "field" }, el("span", {}, label), input);
  const input = (key, type, attrs = {}) => el("input", { type, value: p[key], ...attrs,
    onchange: (e) => { p[key] = type === "number" ? Number(e.target.value) : e.target.value; run(); } });
  const leverage = input("max_leverage", "number", { step: 0.5, min: 0.5, max: 5 });
  // Levier 1 par défaut, y compris en perpétuel : le levier se choisit à la main.
  const onChange = (key) => {
    if (key === "market") {
      p.max_leverage = 1;
      leverage.value = p.max_leverage;
    }
    run();
  };
  const halt = el("input", { type: "checkbox", checked: p.halt_on_breach,
    onchange: (e) => { p.halt_on_breach = e.target.checked; run(); } });

  main.replaceChildren(
    el("h1", {}, `${ind.icon} ${ind.name}`),
    el("p", { class: "muted" }, ind.description),
    tradingControls(p, onChange),
    el("div", { class: "controls" },
      field("Date de début", input("start", "date")),
      field("Date de fin (exclue)", input("end", "date")),
      field("Capital initial (USDT)", input("initial_capital", "number", { step: 100, min: 1 })),
      field("Volatilité visée (%/an)", input("target_vol_pct", "number", { step: 5, min: 1, max: 100 })),
      field("Levier maximal (x)", leverage)),
    el("div", { class: "controls" },
      field("Stop suiveur (ATR, 0 = aucun)", input("stop_atr_mult", "number", { step: 0.5, min: 0, max: 10 })),
      field("Seuil de rééquilibrage (%)", input("rebalance_pct", "number", { step: 5, min: 0, max: 100 })),
      field("Frais par ordre (%)", input("fee_pct", "number", { step: 0.005, min: 0, max: 1 })),
      field("Glissement (%)", input("slippage_pct", "number", { step: 0.01, min: 0, max: 1 })),
      field("Funding supposé (% / 8 h)", input("funding_pct_8h", "number", { step: 0.005, min: -1, max: 1 }))),
    el("div", { class: "controls" }, el("label", {}, halt, " Arrêter au premier coupe-circuit (perte de 3 % sur un jour ou de 10 % depuis le plus haut), comme le fera le moteur en réel")),
    el("p", { class: "muted" }, "Chaque ordre est décidé à une clôture et exécuté à l'ouverture suivante, au prix glissé, frais déduits. Poloniex ne fournit l'historique du funding que sur 180 jours : au-delà, le funding supposé s'applique (perpétuel uniquement)."),
    results,
  );
  runBacktest(ind, results);
}

async function runBacktest(ind, box) {
  const id = ++state.requestId;
  const p = state.bt;
  if (p.start >= p.end) {
    box.replaceChildren(notice("error", "La date de début doit être antérieure à la date de fin."));
    return;
  }
  box.replaceChildren(loading("Backtest en cours (chargement des bougies Poloniex)…"));
  let res;
  try {
    res = await invoke("run_backtest", { params: p });
  } catch (e) {
    if (id === state.requestId) box.replaceChildren(notice("error", String(e)));
    return;
  }
  if (id !== state.requestId) return;

  const s = res.summary;
  const st = s.strategy;
  const bh = s.buy_hold;
  box.replaceChildren();
  res.warnings.forEach((w) => box.append(notice("warning", w)));
  if (s.liquidated) box.append(notice("error", "💀 Position liquidée : le capital ne couvrait plus la marge de maintenance. Le backtest s'arrête à cette date."));
  const btc = s.btc;
  // Le nombre de BTC d'abord : c'est la mesure de réussite. Référence : garder ses BTC (×1,00).
  box.append(el("h2", {}, "🪙 En nombre de BTC"), el("div", { class: "metrics" },
    metricCard("BTC à la fin", `${fmtBtc(btc.end)} BTC`, `Garder ses BTC : ${fmtBtc(btc.start)} BTC`, (btc.multiplier - 1) * 100, true),
    metricCard("Multiplicateur vs garder", `×${fmtNumber(btc.multiplier, 2)}`, "Garder ses BTC = ×1,00", null, true),
    metricCard("BTC gagnés ou perdus", `${fmtSignedBtc(btc.gained)} BTC`, `Du ${s.first_date} au ${s.last_date}`, null, true),
    metricCard("Pire baisse du nombre de BTC", fmtPct(btc.max_drawdown_pct), "Du plus haut au plus bas de la série en BTC"),
    metricCard("Temps hors BTC (en USDT)", fmtPct(btc.time_out_pct, 0), "Part des bougies sans aucune position")));
  const total = s.annualized
    ? metricCard("Rendement annualisé", fmtPct(st.cagr_pct), `Buy & Hold : ${fmtPct(bh.cagr_pct)}`)
    : metricCard("Rendement total (non annualisé)", fmtPct(st.total_return_pct), `Buy & Hold : ${fmtPct(bh.total_return_pct)} · période trop courte pour annualiser`);
  box.append(el("h2", {}, "💵 En USDT"), el("div", { class: "metrics" },
    metricCard("Capital final (USDT)", fmtNumber(st.final_equity), `Du ${s.first_date} au ${s.last_date}`, st.total_return_pct),
    total,
    metricCard("Drawdown maximal", fmtPct(st.max_drawdown_pct), `Buy & Hold : ${fmtPct(bh.max_drawdown_pct)}`),
    metricCard("Sharpe", fmtRatio(st.sharpe), `Buy & Hold : ${fmtRatio(bh.sharpe)} · Sortino : ${fmtRatio(st.sortino)}`),
    metricCard("Ordres", String(s.fills), `${s.round_trips} allers-retours, ${s.win_rate_pct == null ? "–" : fmtPct(s.win_rate_pct, 0)} gagnants, ${s.stops} stops`),
    metricCard("Frais + funding (USDT)", fmtNumber(s.fees + s.funding), `Frais : ${fmtNumber(s.fees)} · funding : ${fmtNumber(s.funding)}`),
    metricCard("Temps investi", fmtPct(s.exposure_pct, 0), `Levier moyen : ${fmtNumber(s.avg_leverage)}x · rotation : ${fmtNumber(s.turnover, 1)}x/an`)));
  if (p.market === "perp") {
    box.append(notice("info", `Funding : réel sur ${s.funding_real_periods} paiements (180 derniers jours), supposé à ${fmtNumber(p.funding_pct_8h, 3)} % par 8 h sur ${s.funding_estimated_periods} paiements. Montant négatif : funding reçu.`));
  }
  if (s.circuit_breakers) {
    box.append(notice("warning", `Coupe-circuit : ${s.circuit_breakers} épisode${s.circuit_breakers > 1 ? "s" : ""} (la baisse de 10 % depuis le plus haut ne se redéclenche qu'après un nouveau plus haut du capital).${s.halted ? " Le backtest s'est arrêté au premier, comme le moteur en réel." : " Cochez « Arrêter au premier coupe-circuit » pour voir l'effet de l'arrêt."}`));
  }
  plot(box, res.chart, scaleFor(ind), ["yaxis", "yaxis2"]);
  if (ind.interpretation) box.append(notice("info", `💡 ${ind.interpretation}`));

  const table = (headers, rows) => el("div", { class: "table-wrap" }, el("table", {},
    el("thead", {}, el("tr", {}, headers.map((h) => el("th", {}, h)))),
    el("tbody", {}, rows.map((cells) => el("tr", {}, cells.map((c) => el("td", {}, c)))))));
  if (res.events.length) {
    box.append(el("h2", {}, "🛡️ Événements de risque"),
      table(["Date", "Type", "Détail"], res.events.map((e) => [e.date, e.kind, e.detail])));
  }
  if (res.fills.length) {
    box.append(el("h2", {}, "📝 Journal des ordres"), table(
      ["Date", "Origine", "Sens", "Prix", "Quantité", "Montant (USDT)", "Frais (USDT)", "Exposition après", "Détail"],
      res.fills.map((f) => [f.date, f.kind, f.side, fmtSig(f.price), fmtSig(f.quantity), fmtNumber(f.notional),
        fmtNumber(f.fee), `${fmtNumber(f.exposure_after, 2)}x`, f.detail])));
  }

  const status = el("div");
  const exportAs = async (format) => {
    try {
      const path = await invoke("export_backtest", { format });
      if (path) status.replaceChildren(notice("success", `Fichier enregistré : ${path}`));
    } catch (e) { status.replaceChildren(notice("error", String(e))); }
  };
  box.append(el("h2", {}, "📥 Exporter les résultats"), el("div", { class: "row" },
    el("button", { class: "btn", type: "button", onclick: () => exportAs("equity") }, "Courbe de capital et en BTC (CSV)"),
    el("button", { class: "btn", type: "button", onclick: () => exportAs("fills") }, "Journal des ordres (CSV)")), status);
}

// ---------- Accumulation BTC (achats au comptant, jamais de vente) ----------

function renderAccumulation(ind) {
  const p = state.acc || (state.acc = {
    start: "2018-01-01", end: today(), base_amount: 100,
    frequency: "weekly", weekday: 1, month_day: 1,
    weighting: "mvrv_tiers",
    tier_bounds: [1, 2, 3], tier_multipliers: [3, 1.5, 1, 0.5], max_multiplier: 3,
    fee_pct: 0.075, slippage_pct: 0.05, initial_btc: 0,
  });
  const results = el("div");
  const run = debounce(() => runAccumulation(ind, results), 400);
  const field = (label, input) => el("label", { class: "field" }, el("span", {}, label), input);
  const num = (key, attrs = {}) => el("input", { type: "number", value: p[key], ...attrs,
    onchange: (e) => { p[key] = Number(e.target.value); run(); } });
  const date = (key) => el("input", { type: "date", value: p[key], onchange: (e) => { p[key] = e.target.value; run(); } });
  const select = (key, options, after) => el("select", { onchange: (e) => {
    p[key] = typeof p[key] === "number" ? Number(e.target.value) : e.target.value;
    if (after) after();
    run();
  } }, options.map(([value, label]) => el("option", { value, selected: value === p[key] }, label)));

  // Les champs qui ne concernent pas le choix courant sont masqués.
  const weekdayField = field("Jour de la semaine", select("weekday", WEEKDAYS));
  const monthField = field("Jour du mois (31 : dernier jour)", num("month_day", { step: 1, min: 1, max: 31 }));
  const tiersBox = el("div", { class: "controls" });
  const sync = () => {
    weekdayField.hidden = p.frequency !== "weekly";
    monthField.hidden = p.frequency !== "monthly";
    tiersBox.hidden = p.weighting !== "mvrv_tiers";
  };
  const tierField = (label, arr, i, attrs) => field(label, el("input", { type: "number", value: arr[i], ...attrs,
    onchange: (e) => { arr[i] = Number(e.target.value); run(); } }));
  tiersBox.append(
    ...p.tier_bounds.map((_, i) => tierField(`Palier ${i + 1} : MVRV <`, p.tier_bounds, i, { step: 0.1, min: 0.1 })),
    ...p.tier_multipliers.map((_, i) => tierField(
      i < p.tier_bounds.length ? `Multiplicateur du palier ${i + 1}` : `Multiplicateur à partir du palier ${p.tier_bounds.length}`,
      p.tier_multipliers, i, { step: 0.1, min: 0, max: 100 })));

  main.replaceChildren(
    el("h1", {}, `${ind.icon} ${ind.name}`),
    el("p", { class: "muted" }, ind.description),
    el("div", { class: "controls" },
      field("Début", date("start")),
      field("Fin (incluse)", date("end")),
      field("Montant de base par achat (USDT)", num("base_amount", { step: 10, min: 1 })),
      field("Fréquence", select("frequency", [["weekly", "Hebdomadaire"], ["monthly", "Mensuelle"]], sync)),
      weekdayField, monthField),
    el("div", { class: "controls" },
      field("Pondération", select("weighting", WEIGHTINGS, sync)),
      field("Multiplicateur maximal (×)", num("max_multiplier", { step: 0.5, min: 1, max: 100 })),
      field("Frais par ordre (%)", num("fee_pct", { step: 0.005, min: 0, max: 1 })),
      field("Glissement (%)", num("slippage_pct", { step: 0.01, min: 0, max: 1 })),
      field("Stack de BTC initial (facultatif)", num("initial_btc", { step: 0.01, min: 0 }))),
    tiersBox,
    el("p", { class: "muted" }, "Le poids d'un achat utilise la dernière clôture journalière complète et le MVRV daté de la veille au plus tard ; l'ordre est exécuté à l'ouverture du jour, au prix glissé, frais déduits du montant. Le module ne vend jamais. Pas de règle « tout ou rien » : un MVRV élevé réduit l'achat sans l'annuler."),
    results,
  );
  sync();
  runAccumulation(ind, results);
}

async function runAccumulation(ind, box) {
  const id = ++state.requestId;
  const p = state.acc;
  if (p.start > p.end) {
    box.replaceChildren(notice("error", "La date de début ne peut pas suivre la date de fin."));
    return;
  }
  box.replaceChildren(loading("Simulation en cours (bougies Poloniex et MVRV)…"));
  let res;
  try {
    res = await invoke("run_accumulation", { params: p });
  } catch (e) {
    if (id === state.requestId) box.replaceChildren(notice("error", String(e)));
    return;
  }
  if (id !== state.requestId) return;

  const s = res.summary;
  const eb = s.equal_budget;
  const fb = s.fixed_base;
  box.replaceChildren();
  res.warnings.forEach((w) => box.append(notice("warning", w)));
  // Chiffre principal : BTC en plus face à un DCA fixe qui dépense exactement le même total.
  box.append(el("div", { class: "metrics" },
    metricCard("BTC en plus à budget égal", `${fmtSignedBtc(eb.extra_btc)} BTC`,
      `Face à un DCA fixe de ${fmtNumber(eb.spent, 0)} USDT en montants égaux, aux mêmes dates (${fmtBtc(eb.btc)} BTC)`,
      eb.extra_pct, true),
    metricCard("USDT dépensés", fmtNumber(s.spent, 0), `${s.purchases} achats du ${s.first_date} au ${s.last_date} · poids moyen ×${fmtNumber(s.avg_weight, 2)}, maximal ×${fmtNumber(s.max_weight, 2)}`),
    metricCard("BTC accumulés", `${fmtBtc(s.btc_bought)} BTC`,
      s.initial_btc > 0 ? `Stack final avec les ${fmtBtc(s.initial_btc)} BTC de départ : ${fmtBtc(s.btc_total)} BTC` : `Valeur à la dernière clôture : ${fmtNumber(s.value_usdt, 0)} USDT`),
    metricCard("Coût moyen par BTC", s.avg_cost == null ? "–" : `${fmtNumber(s.avg_cost, 0)} USDT`,
      `DCA fixe, même budget : ${eb.avg_cost == null ? "–" : fmtNumber(eb.avg_cost, 0)} USDT`)));
  box.append(el("div", { class: "metrics" },
    metricCard("Face à un DCA fixe au même montant de base", `${fmtSignedBtc(fb.extra_btc)} BTC`,
      `Budget total différent : ${fmtNumber(fb.spent, 0)} USDT dépensés par la référence, contre ${fmtNumber(s.spent, 0)} USDT ici (${fmtSignedPct(fb.extra_pct)} de BTC)`),
    metricCard("Frais payés", `${fmtNumber(s.fees)} USDT`, s.skipped ? `${s.skipped} achats prévus non simulés` : null),
    s.fallback_purchases > 0 && metricCard("Achats en secours", String(s.fallback_purchases), "Pondérés par prix / moyenne 200 j faute de MVRV récent")));
  plot(box, res.chart, scaleFor(ind));
  if (ind.interpretation) box.append(notice("info", `💡 ${ind.interpretation}`));

  if (res.mvrv) {
    const m = res.mvrv;
    box.append(el("p", { class: "muted" },
      `MVRV : Coin Metrics jusqu'au ${m.coinmetrics_last ?? "–"}, BGeometrics jusqu'au ${m.bgeometrics_last ?? "–"} (la plus récente l'emporte sur les dates communes). `,
      m.overlap_days > 0 ? `Contrôle sur ${m.overlap_days} jours communs : écart moyen ${fmtPct(m.mean_gap_pct)}, maximal ${fmtPct(m.max_gap_pct)}.` : "Aucune date commune pour contrôler l'écart."));
  }
  box.append(el("h2", {}, "🧾 Journal des achats"), el("div", { class: "table-wrap" }, el("table", {},
    el("thead", {}, el("tr", {}, ["Date", "Poids", "Origine", "MVRV", "Prix / moy. 200 j", "Montant (USDT)", "Prix d'exécution", "BTC reçus"].map((h) => el("th", {}, h)))),
    el("tbody", {}, res.purchases.map((b) => el("tr", {},
      [b.date, `×${fmtNumber(b.weight, 2)}`, b.source, b.mvrv == null ? "–" : fmtNumber(b.mvrv, 2),
        b.ma_ratio == null ? "–" : fmtNumber(b.ma_ratio, 2), fmtNumber(b.amount), fmtSig(b.price), fmtBtc(b.btc)].map((c) => el("td", {}, c))))))));
}

// ---------- Fibonacci à levier (backtest, aucun ordre réel) ----------

const FIBO_INTERVALS = [["MINUTE_15", "15 minutes"], ["HOUR_1", "1 heure"], ["HOUR_4", "4 heures"]];
const FIBO_RATIOS = [0.382, 0.5, 0.618, 0.786];
const STOP_KINDS = [["next_level", "Au-delà du niveau suivant"], ["origin", "Au-delà de l'origine A"], ["fixed", "Distance fixe"]];
const TARGET_KINDS = [["return_to_b", "Retour à B (0 %)"], ["ext_1272", "Extension 127,2 %"], ["ext_1618", "Extension 161,8 %"], ["r_multiple", "Multiple du risque"]];
const FIBO_WARNING = "Le levier ne crée pas d'avantage. À x75, une variation de 0,8 % liquide la position. Les résultats passés ne garantissent rien.";

function fiboDefaults() {
  return {
    base: "BTC",
    fibo: { interval: "HOUR_1", threshold_mode: "atr", atr_mult: 3, threshold_pct: 1, ratios: [...FIBO_RATIOS], direction: "continuation", max_age_bars: 48 },
    rules: { stop: { kind: "next_level", margin_pct: 0 }, target: { kind: "return_to_b" }, prudent_tick: false, liquidation_as_stop: false, max_hold_bars: 0 },
    costs: { fees: { maker_pct: 0.02, taker_pct: 0.05 }, slippage_pct: 0.01, funding_pct_8h: 0.01, liquidation_safety_pct: 0.1 },
    account: { initial_budget: 500, margin: 10, leverage: 20 },
    study_start: "2023-01-01", study_end: "2024-12-31",
    test_start: "2025-01-01", test_end: today(),
    placebo: { draws: 200, seed: 20260101, low: 0.3, high: 0.8, fixed_levels: [0.45, 0.55, 0.7] },
    mc_draws: 10000,
    use_mark_price: false,
  };
}

const fmtUtc = (ms) => `${new Date(ms).toISOString().slice(0, 16).replace("T", " ")} UTC`;
const fmtR = (v) => (v == null ? "–" : `${v > 0 ? "+" : v < 0 ? "−" : ""}${fmtNumber(Math.abs(v), 3)} R`);
const fmtUsd = (v, d = 2) => (v == null ? "–" : `${v > 0 ? "+" : v < 0 ? "−" : ""}${fmtNumber(Math.abs(v), d)}`);
const dataTable = (headers, rows) => el("div", { class: "table-wrap" }, el("table", {},
  el("thead", {}, el("tr", {}, headers.map((h) => el("th", {}, h)))),
  el("tbody", {}, rows.map((cells) => el("tr", {}, cells.map((c) => el("td", {}, c)))))));
const fieldOf = (label, input, hint) => el("label", { class: "field", title: hint || null }, el("span", {}, label), input);

let fiboUnlisten = null;

function renderFibo(ind) {
  const p = state.fibo || (state.fibo = fiboDefaults());
  const progressBox = el("div");
  const dataBox = el("div");
  const pointsBox = el("div");
  const results = el("div");
  const excursionBox = el("div");
  const runPoints = debounce(() => loadFiboPoints(ind, pointsBox), 500);

  // Progression des téléchargements et des calculs (événement du moteur).
  if (fiboUnlisten) { fiboUnlisten(); fiboUnlisten = null; }
  window.__TAURI__.event.listen("fibo-progress", (e) => showFiboProgress(progressBox, e.payload)).then((u) => { fiboUnlisten = u; });

  const num = (obj, key, attrs = {}, after) => el("input", { type: "number", value: obj[key], ...attrs,
    onchange: (e) => { obj[key] = Number(e.target.value); if (after) after(); } });
  const date = (obj, key) => el("input", { type: "date", value: obj[key], onchange: (e) => { obj[key] = e.target.value; } });
  const select = (obj, key, options, after) => el("select", { onchange: (e) => { obj[key] = e.target.value; if (after) after(); } },
    options.map(([v, l]) => el("option", { value: v, selected: v === obj[key] }, l)));
  const check = (obj, key, label, after) => el("label", {}, el("input", { type: "checkbox", checked: obj[key],
    onchange: (e) => { obj[key] = e.target.checked; if (after) after(); } }), ` ${label}`);

  // Champs qui dépendent d'un choix : seuil, stop, objectif.
  const atrField = fieldOf("k de k × ATR(14)", num(p.fibo, "atr_mult", { step: 0.5, min: 0.5, max: 20 }, runPoints));
  const pctField = fieldOf("Seuil (%)", num(p.fibo, "threshold_pct", { step: 0.1, min: 0.05, max: 50 }, runPoints));
  const stopSel = el("select", { onchange: (e) => setStop(e.target.value) },
    STOP_KINDS.map(([v, l]) => el("option", { value: v, selected: v === p.rules.stop.kind }, l)));
  const stopVal = el("input", { type: "number", step: 0.1, min: 0, max: 50, value: p.rules.stop.margin_pct ?? p.rules.stop.pct ?? 0,
    onchange: () => setStop(stopSel.value) });
  const stopLabel = el("span", {}, "");
  const setStop = (kind) => {
    const v = Number(stopVal.value);
    p.rules.stop = kind === "fixed" ? { kind, pct: v || 1 } : { kind, margin_pct: v };
    stopLabel.textContent = kind === "fixed" ? "Distance du stop (%)" : "Marge au-delà (%)";
  };
  const targetSel = el("select", { onchange: (e) => setTarget(e.target.value) },
    TARGET_KINDS.map(([v, l]) => el("option", { value: v, selected: v === targetKey(p.rules.target) }, l)));
  const targetVal = el("input", { type: "number", step: 0.5, min: 0.5, max: 50, value: p.rules.target.r ?? 2, onchange: () => setTarget(targetSel.value) });
  const targetValField = el("label", { class: "field" }, el("span", {}, "Multiple du risque (R)"), targetVal);
  function targetKey(t) { return t.kind === "extension" ? (t.ratio > 1.4 ? "ext_1618" : "ext_1272") : t.kind; }
  const setTarget = (key) => {
    p.rules.target = key === "ext_1272" ? { kind: "extension", ratio: 1.272 }
      : key === "ext_1618" ? { kind: "extension", ratio: 1.618 }
      : key === "r_multiple" ? { kind: "r_multiple", r: Number(targetVal.value) || 2 } : { kind: "return_to_b" };
    targetValField.hidden = key !== "r_multiple";
  };
  const syncThreshold = () => { atrField.hidden = p.fibo.threshold_mode !== "atr"; pctField.hidden = p.fibo.threshold_mode !== "percent"; };

  const ratioBoxes = el("div", { class: "controls" }, el("span", { class: "muted" }, "Niveaux :"),
    FIBO_RATIOS.map((r) => el("label", {}, el("input", { type: "checkbox", checked: p.fibo.ratios.includes(r), onchange: (e) => {
      p.fibo.ratios = e.target.checked ? [...p.fibo.ratios, r].sort() : p.fibo.ratios.filter((x) => x !== r);
      runPoints();
    } }), ` ${fmtNumber(r * 100, 1)} %`)));

  const leverage = num(p.account, "leverage", { step: 1, min: 1, max: 75 });
  const runBtn = el("button", { class: "btn", type: "button", onclick: () => runFibo(ind, results, progressBox, runBtn) }, "▶ Lancer l'étude (périodes d'étude et de test)");
  const syncBtn = el("button", { class: "btn", type: "button", onclick: () => syncFibo(progressBox, dataBox, syncBtn) }, "⬇ Télécharger les bougies 1 min manquantes");
  const cancelBtn = el("button", { class: "btn", type: "button", onclick: () => invoke("fibo_cancel") }, "✖ Annuler");

  main.replaceChildren(
    el("h1", {}, `${ind.icon} ${ind.name}`),
    notice("warning", `⚠️ ${FIBO_WARNING}`),
    el("p", { class: "muted" }, ind.description),
    el("h2", {}, "🗄️ Données et contrat"), dataBox,
    el("div", { class: "controls" }, check(p, "use_mark_price", "Utiliser l'historique du prix de marque pour les liquidations (s'il est disponible)"), syncBtn, cancelBtn),
    progressBox,

    el("h2", {}, "⚙️ Identification des points"),
    el("div", { class: "controls" },
      fieldOf("Unité de détection", select(p.fibo, "interval", FIBO_INTERVALS, runPoints)),
      fieldOf("Seuil de retournement", select(p.fibo, "threshold_mode", [["atr", "k × ATR(14)"], ["percent", "Pourcentage"]], () => { syncThreshold(); runPoints(); })),
      atrField, pctField,
      fieldOf("Sens des trades", select(p.fibo, "direction", [["continuation", "Continuation (acheter le repli, vendre le rebond)"], ["contrary", "Contraire"]], runPoints)),
      fieldOf("Durée de vie d'une mise en place (bougies)", num(p.fibo, "max_age_bars", { step: 1, min: 1, max: 1000 }, runPoints))),
    ratioBoxes,

    el("h2", {}, "🎯 Règles de trade"),
    el("div", { class: "controls" },
      fieldOf("Stop", stopSel), el("label", { class: "field" }, stopLabel, stopVal),
      fieldOf("Objectif", targetSel), targetValField,
      fieldOf("Durée maximale d'un trade (bougies, 0 = aucune)", num(p.rules, "max_hold_bars", { step: 1, min: 0 }))),
    el("div", { class: "controls" },
      check(p.rules, "prudent_tick", "Prudent : traverser le niveau d'un tick pour être exécuté", runPoints),
      check(p.rules, "liquidation_as_stop", "Si la liquidation tombe avant le stop : elle devient le stop (sinon le trade est refusé et compté)")),

    el("h2", {}, "💰 Taille et coûts"),
    el("div", { class: "controls" },
      fieldOf("Budget de trading initial (USDT)", num(p.account, "initial_budget", { step: 50, min: 1 })),
      fieldOf("Marge isolée par trade (USDT)", num(p.account, "margin", { step: 1, min: 1 })),
      fieldOf("Levier (1 à maximum)", leverage),
      fieldOf("Frais maker (%) — à vérifier", num(p.costs.fees, "maker_pct", { step: 0.005, min: 0, max: 1 })),
      fieldOf("Frais taker (%) — à vérifier", num(p.costs.fees, "taker_pct", { step: 0.005, min: 0, max: 1 })),
      fieldOf("Glissement des sorties au marché (%)", num(p.costs, "slippage_pct", { step: 0.005, min: 0, max: 2 })),
      fieldOf("Funding supposé (% / 8 h)", num(p.costs, "funding_pct_8h", { step: 0.005, min: -1, max: 1 })),
      fieldOf("Marge de sécurité de liquidation (%)", num(p.costs, "liquidation_safety_pct", { step: 0.05, min: 0, max: 5 }, null),
        "Sans historique du prix de marque : la liquidation est déclenchée plus tôt de ce pourcentage du prix.")),

    el("h2", {}, "🧪 Validation"),
    el("div", { class: "controls" },
      fieldOf("Étude : début", date(p, "study_start")), fieldOf("Étude : fin", date(p, "study_end")),
      fieldOf("Test : début", date(p, "test_start")), fieldOf("Test : fin", date(p, "test_end")),
      fieldOf("Tirages placebo", num(p.placebo, "draws", { step: 50, min: 1, max: 5000 })),
      fieldOf("Graine", num(p.placebo, "seed", { step: 1 })),
      fieldOf("Niveaux placebo : min", num(p.placebo, "low", { step: 0.05, min: 0.05, max: 0.95 })),
      fieldOf("Niveaux placebo : max", num(p.placebo, "high", { step: 0.05, min: 0.05, max: 0.95 })),
      fieldOf("Tirages Monte-Carlo", num(p, "mc_draws", { step: 1000, min: 100 }))),
    el("p", { class: "muted" }, "Les réglages se choisissent sur la période d'étude ; le verdict ne se lit que sur la période de test, postérieure et jamais utilisée pour régler. Le placebo rejoue le même moteur avec des niveaux tirés au hasard (graine fixe)."),
    el("div", { class: "row" }, runBtn),
    results,

    el("h2", {}, "📍 Points actuels"),
    el("p", { class: "muted" }, "Dernier mouvement confirmé d'après les bougies récentes de Poloniex : un pivot n'apparaît qu'une fois confirmé, jamais avant."),
    pointsBox,

    el("h2", {}, "🔎 Contrôle des repères journaliers"),
    el("p", { class: "muted" }, "Part des jours où le prix s'éloigne de l'ouverture d'au moins une distance donnée (bougies journalières BTC_USDT du 01/05 au 12/10/2026), comparée à vos repères. Ces distances correspondent environ à la liquidation à x100, x75, x50 et x20."),
    el("div", { class: "row" }, el("button", { class: "btn", type: "button", onclick: () => loadExcursions(excursionBox) }, "Contrôler les repères")),
    excursionBox,
  );
  syncThreshold();
  setStop(p.rules.stop.kind);
  setTarget(targetKey(p.rules.target));
  loadFiboOverview(dataBox, leverage);
  loadFiboPoints(ind, pointsBox);
}

function showFiboProgress(box, p) {
  if (!p) return;
  const pct = p.total ? Math.round((p.done / p.total) * 100) : null;
  box.replaceChildren(el("div", { class: "card" },
    el("div", {}, `${p.phase}${p.total ? ` : ${p.done} / ${p.total}` : ""}${p.at ? ` · ${p.at}` : ""}${p.bars ? ` · ${fmtNumber(p.bars, 0)} bougies reçues` : ""}`),
    p.total ? el("progress", { max: p.total, value: p.done, style: "width:100%" }) : el("div", { class: "muted" }, "…")));
}

async function loadFiboOverview(box, leverageInput) {
  box.replaceChildren(loading("Lecture des paramètres du contrat…"));
  let o;
  try { o = await invoke("fibo_overview"); } catch (e) { box.replaceChildren(notice("error", String(e))); return; }
  box.replaceChildren();
  if (o.contract) {
    const c = o.contract;
    leverageInput.max = c.max_leverage;
    box.append(el("div", { class: "metrics" },
      metricCard("Contrat", c.symbol, `Valeur d'un contrat : ${fmtNumber(c.contract_value, 4)} · pas ${fmtNumber(c.lot_size, 0)} · minimum ${fmtNumber(c.min_size, 0)} · tick ${fmtNumber(c.tick_size, 2)}`),
      metricCard("Levier maximal", `x${fmtNumber(c.max_leverage, 0)}`, "Lu sur l'API : tout levier au-delà est refusé"),
      metricCard("Marge de maintenance", c.tiers.map((t) => fmtPct(t.rate * 100, 2)).join(" · "), c.tiers_from_risk_limit ? "Paliers lus sur l'API" : "Taux unique de la fiche du contrat")));
    c.warnings.forEach((w) => box.append(notice("warning", w)));
  } else {
    box.append(notice("error", `Paramètres du contrat indisponibles : ${o.contract_error}`));
  }
  showFiboStatus(box, o.status);
}

function showFiboStatus(box, status) {
  box.querySelectorAll(".fibo-status").forEach((n) => n.remove());
  box.append(el("div", { class: "fibo-status muted" },
    `Cache disque (${status.cache_dir}) : ${fmtNumber(status.cache_bytes / 1e6, 1)} Mo. `,
    status.series.map((s) => `${s.label} : ${s.first ? `${s.first} → ${s.last}` : "vide"}`).join(" · ")));
}

async function syncFibo(progressBox, dataBox, btn) {
  const p = state.fibo;
  btn.disabled = true;
  progressBox.replaceChildren(loading("Téléchargement des bougies 1 minute (reprenable, annulable)…"));
  try {
    const r = await invoke("fibo_sync", { request: p });
    progressBox.replaceChildren(notice(r.cancelled ? "info" : "success", r.cancelled ? "Téléchargement annulé : ce qui est reçu est conservé, relancez pour reprendre." : "Bougies 1 minute à jour."));
    r.warnings.forEach((w) => progressBox.append(notice("warning", w)));
    showFiboStatus(dataBox, r.status);
  } catch (e) {
    progressBox.replaceChildren(notice("error", String(e)));
  } finally { btn.disabled = false; }
}

async function loadFiboPoints(ind, box) {
  const id = ++state.requestId;
  box.replaceChildren(loading("Chargement des bougies récentes…"));
  let r;
  try {
    r = await invoke("fibo_points", { params: state.fibo.fibo, prudentTick: state.fibo.rules.prudent_tick });
  } catch (e) {
    if (id === state.requestId) box.replaceChildren(notice("error", String(e)));
    return;
  }
  if (id !== state.requestId) return;
  box.replaceChildren();
  const c = r.current;
  box.append(el("div", { class: "metrics" },
    metricCard("Unité de détection", r.interval, `ATR(14) : ${r.atr == null ? "–" : fmtSig(r.atr)} · seuil de retournement : ${r.threshold == null ? "–" : fmtSig(r.threshold)} USDT`)));
  if (!c) { box.append(notice("info", "Aucun mouvement confirmé dans les bougies chargées.")); }
  else {
    box.append(el("div", { class: "metrics" },
      metricCard("Dernier mouvement confirmé", c.up ? "Haussier ▲" : "Baissier ▼", `A (${c.up ? "creux" : "sommet"}) : ${fmtSig(c.a_price)} le ${fmtUtc(c.a_time)}`),
      metricCard("Point B", fmtSig(c.b_price), `${c.up ? "Sommet" : "Creux"} du ${fmtUtc(c.b_time)}`),
      metricCard("Confirmé le", fmtUtc(c.confirmed_time), `Sens des trades : ${c.side === "long" ? "achat" : "vente"} · ${c.active ? "mise en place active" : c.ended}`)));
    const color = { "En attente": "", "Déclenché": "up", "Invalidé": "down" };
    box.append(dataTable(["Niveau", "Prix", "Statut"], c.levels.map((l) => [`${fmtNumber(l.ratio * 100, 1)} %`, fmtSig(l.price),
      el("span", { class: `chip ${color[l.status]}` }, l.status)])));
    if (c.ignored.length) box.append(el("p", { class: "muted" }, `Niveaux déjà dépassés à la confirmation, ignorés : ${c.ignored.map((x) => `${fmtNumber(x * 100, 1)} %`).join(", ")}.`));
  }
  plot(box, r.chart, scaleFor(ind), ["yaxis"]);
}

async function loadExcursions(box) {
  box.replaceChildren(loading("Chargement des bougies journalières…"));
  try {
    const r = await invoke("fibo_excursions");
    box.replaceChildren(el("p", { class: "muted" }, `${r.days} jours, du ${r.first} au ${r.last}.`),
      dataTable(["Distance", "Hausse (mesure / repère)", "Baisse (mesure / repère)", "Les deux (mesure / repère)", "Écart maximal"],
        r.rows.map((x) => [`${fmtNumber(x.measured.distance_pct, 2)} %`,
          `${fmtNumber(x.measured.up_pct, 0)} % / ${fmtNumber(x.reference_up, 0)} %`,
          `${fmtNumber(x.measured.down_pct, 0)} % / ${fmtNumber(x.reference_down, 0)} %`,
          `${fmtNumber(x.measured.both_pct, 0)} % / ${fmtNumber(x.reference_both, 0)} %`,
          `${fmtNumber(x.max_gap, 1)} pt`])));
  } catch (e) { box.replaceChildren(notice("error", String(e))); }
}

async function runFibo(ind, box, progressBox, btn) {
  const p = state.fibo;
  btn.disabled = true;
  box.replaceChildren(loading("Étude en cours : simulation, niveaux placebo, Monte-Carlo (peut durer plusieurs dizaines de secondes)…"));
  let r;
  try {
    r = await invoke("fibo_run", { request: p });
  } catch (e) {
    box.replaceChildren(notice("error", String(e)));
    btn.disabled = false;
    return;
  }
  btn.disabled = false;
  progressBox.replaceChildren();
  box.replaceChildren();
  r.warnings.forEach((w) => box.append(notice("warning", w)));

  // Verdict : lu sur la période de test.
  const v = r.verdict;
  const cls = v.verdict === "Avantage démontré sur la période de test" ? "success" : "warning";
  box.append(el("h2", {}, "⚖️ Verdict (période de test)"),
    notice(cls, `${v.label}.`),
    dataTable(["Critère", "Valeur", ""], v.criteria.map((c) => [c.name, c.value, c.ok ? "✓" : "✗"])));

  const t = r.test;
  const s = t.summary;
  const st = t.stats;
  box.append(el("h2", {}, "📊 Période de test"), el("div", { class: "metrics" },
    metricCard("Mises en place", String(st.setups), `${st.placed} posées · ${st.levels_ignored} niveaux déjà dépassés · ${st.skipped_busy} ignorées (position ouverte)`),
    metricCard("Trades", String(s.trades), `Taux de réussite : ${s.win_rate_pct == null ? "–" : fmtPct(s.win_rate_pct, 1)}`),
    metricCard("Refus", String(st.refused_min_size + st.refused_liquidation + st.refused_other),
      `${st.refused_min_size} moins d'un contrat · ${st.refused_liquidation} liquidation avant le stop · ${st.refused_other} autres`),
    metricCard("Liquidations", String(s.liquidations), "Toute la marge isolée est perdue"),
    metricCard("Bougies ambiguës", String(st.ambiguous), "Stop et objectif dans la même bougie 1 min : sortie défavorable"),
    metricCard("Espérance nette par trade", fmtR(s.expectancy_r_net), `En USDT : ${fmtUsd(s.expectancy_usdt)} · brute : ${fmtR(s.expectancy_r_gross)}`, null, true),
    metricCard("Frais et funding", `${fmtPct(s.fees_pct_of_margin, 1)} + ${fmtPct(s.funding_pct_of_margin, 2)}`, "En % des marges engagées"),
    metricCard("Budget final", `${fmtNumber(s.final_budget)} USDT`, `Initial : ${fmtNumber(s.initial_budget)} · pire baisse : ${fmtPct(s.max_drawdown_pct, 1)}${s.ruined ? ` · RUINE le ${t.ruin_time}` : ""}`)));
  if (s.ruined) box.append(notice("error", `💀 Ruine : la marge d'un trade n'est plus finançable (${t.ruin_time}). La simulation s'arrête.`));

  // Étude et test côte à côte.
  const col = (period) => {
    const m = period.summary;
    return [String(m.trades), m.win_rate_pct == null ? "–" : fmtPct(m.win_rate_pct, 1), fmtR(m.expectancy_r_net), fmtR(m.expectancy_r_gross),
      fmtUsd(m.expectancy_usdt), fmtUsd(m.total_net), fmtPct(m.fees_pct_of_margin, 1), fmtPct(m.funding_pct_of_margin, 2),
      fmtNumber(m.final_budget), fmtPct(m.max_drawdown_pct, 1), String(m.liquidations),
      period.placebo.percentile == null ? "–" : `${fmtNumber(period.placebo.percentile * 100, 1)} %`,
      `${fmtR(period.x1.expectancy_r_net)} (${period.x1.trades} trades)`];
  };
  const labels = ["Trades", "Taux de réussite", "Espérance nette par trade", "Espérance brute par trade", "Espérance (USDT)", "Résultat net (USDT)",
    "Frais (% des marges)", "Funding (% des marges)", "Budget final (USDT)", "Pire baisse du budget", "Liquidations", "Rang de Fibonacci face au placebo", "Même jeu à x1 (R seulement)"];
  const [cs, ct] = [col(r.study), col(t)];
  box.append(el("h2", {}, "↔️ Étude et test côte à côte"),
    dataTable(["", `Étude (${r.study.start} → ${r.study.end})`, `Test (${t.start} → ${t.end})`], labels.map((l, i) => [l, cs[i], ct[i]])),
    el("p", { class: "muted" }, "À x1, le même moteur (quantité fractionnaire, sans liquidation ni arrondi aux contrats) donne la même espérance en R : le levier change la taille de la position et les frais rapportés à la marge, pas l'avantage."));

  // Détail par niveau.
  const levelRows = (rows) => rows.map((l) => [`${fmtNumber(l.ratio * 100, 1)} %`, String(l.trades), l.win_rate_pct == null ? "–" : fmtPct(l.win_rate_pct, 1), fmtR(l.expectancy_r_net), fmtUsd(l.total_net)]);
  const levelHeaders = ["Niveau", "Trades", "Réussite", "Espérance nette", "Net (USDT)"];
  box.append(el("h2", {}, "📐 Détail par niveau (période de test)"),
    el("h3", {}, "Niveaux de Fibonacci"), dataTable(levelHeaders, levelRows(t.per_level)),
    el("h3", {}, `Niveaux fixes comparables (${t.fixed_levels.map((x) => fmtNumber(x, 2)).join(" ; ")})`), dataTable(levelHeaders, levelRows(t.fixed_per_level)));

  // Placebo et Monte-Carlo.
  const pl = t.placebo;
  box.append(el("h2", {}, "🎲 Niveaux placebo"), el("div", { class: "metrics" },
    metricCard("Rang de Fibonacci (test)", pl.percentile == null ? "–" : `${fmtNumber(pl.percentile * 100, 1)} %`, `Part des ${pl.valid_draws} tirages valides qu'il bat strictement`, null, true),
    metricCard("Placebo : moyenne", fmtR(pl.mean), `5 % : ${fmtR(pl.p5)} · médiane : ${fmtR(pl.median)} · 95 % : ${fmtR(pl.p95)}`),
    metricCard("Fibonacci", fmtR(pl.fibo_expectancy_r_net), "Espérance nette par trade")));
  plot(box, t.placebo_chart, null, []);
  plot(box, r.study.placebo_chart, null, []);
  const mc = t.monte_carlo;
  if (mc) {
    box.append(el("h2", {}, "🎰 Monte-Carlo sur la suite des trades (test)"), el("div", { class: "metrics" },
      metricCard("Budget final médian", `${fmtNumber(mc.final_quantiles[2])} USDT`, `5 % : ${fmtNumber(mc.final_quantiles[0])} · 95 % : ${fmtNumber(mc.final_quantiles[4])} · moyenne : ${fmtNumber(mc.final_mean)}`),
      metricCard("Probabilité de perdre 50 %", fmtPct(mc.prob_loss_50 * 100, 1), "Budget passant sous la moitié de son montant initial"),
      metricCard("Probabilité de ruine", fmtPct(mc.prob_ruin * 100, 1), "Perte de tout le budget (marge non finançable)"),
      metricCard("Pire série de pertes", `${fmtNumber(mc.worst_streak_median, 0)} (95 % : ${fmtNumber(mc.worst_streak_p95, 0)})`, `Observée : ${mc.worst_streak_observed} trades · ${fmtNumber(mc.draws, 0)} tirages`)));
    plot(box, t.monte_carlo_chart, null, []);
  }

  // Graphiques.
  box.append(el("h2", {}, "📈 Prix, pivots, niveaux et trades"));
  plot(box, r.price_chart, scaleFor(ind), ["yaxis"]);
  box.append(el("h2", {}, "💼 Budget et résultats en R"));
  plot(box, r.study.budget_chart, null, []);
  plot(box, t.budget_chart, null, []);
  plot(box, r.study.r_chart, null, []);
  plot(box, t.r_chart, null, []);

  // Trades, avec zoom.
  const zoomBox = el("div");
  const tradesFor = (period) => period.trades.map((x, i) => {
    const row = [String(x.id + 1), fmtUtc(x.entry_time), x.side === "long" ? "Achat" : "Vente", `${fmtNumber(x.ratio * 100, 1)} %`, fmtSig(x.entry_price),
      fmtUtc(x.exit_time), fmtSig(x.exit_price), x.reason, `x${fmtNumber(x.leverage, 0)}`, fmtNumber(x.margin), fmtUsd(x.net), fmtR(x.r_net)];
    return { row, i };
  });
  const periodSel = el("select", {}, [["test", "Période de test"], ["study", "Période d'étude"]].map(([v, l]) => el("option", { value: v }, l)));
  const tableBox = el("div");
  const showTrades = () => {
    const period = periodSel.value === "study" ? r.study : t;
    tableBox.replaceChildren(el("div", { class: "table-wrap" }, el("table", {},
      el("thead", {}, el("tr", {}, ["N°", "Entrée", "Sens", "Niveau", "Prix", "Sortie", "Prix", "Motif", "Levier", "Marge", "Net (USDT)", "R net"].map((h) => el("th", {}, h)))),
      el("tbody", {}, tradesFor(period).map(({ row, i }) => el("tr", { style: "cursor:pointer", title: "Cliquer pour zoomer sur ce trade", onclick: async () => {
        zoomBox.replaceChildren(loading("Chargement des bougies 1 minute…"));
        try { const fig = await invoke("fibo_zoom", { period: periodSel.value, index: i }); zoomBox.replaceChildren(); plot(zoomBox, fig, null, []); }
        catch (e) { zoomBox.replaceChildren(notice("error", String(e))); }
      } }, row.map((c) => el("td", {}, c))))))));
  };
  periodSel.addEventListener("change", showTrades);
  box.append(el("h2", {}, "📝 Trades"), el("div", { class: "controls" }, periodSel), tableBox, zoomBox);
  showTrades();

  const status = el("div");
  box.append(el("h2", {}, "📥 Exporter"), el("div", { class: "row" }, el("button", { class: "btn", type: "button", onclick: async () => {
    try { const path = await invoke("fibo_export"); if (path) status.replaceChildren(notice("success", `Fichier enregistré : ${path}`)); }
    catch (e) { status.replaceChildren(notice("error", String(e))); }
  } }, "Trades (CSV)")), status);
  if (ind.interpretation) box.append(notice("info", `💡 ${ind.interpretation}`));
  box.append(notice("warning", `⚠️ ${FIBO_WARNING}`));
}

// ---------- Démarrage ----------

document.querySelectorAll("#scale input").forEach((radio) => radio.addEventListener("change", () => {
  const ind = current();
  if (!ind) return;
  state.scale[ind.id] = radio.value;
  main.querySelectorAll(".chart").forEach((div) => {
    const axes = (div.dataset.axes ?? "yaxis").split(",").filter(Boolean);
    if (!axes.length) return;
    Plotly.relayout(div, Object.fromEntries(axes.map((a) => [`${a}.type`, radio.value])));
  });
}));

(async () => {
  try {
    state.indicators = await invoke("list_indicators");
    state.trading = await invoke("list_trading_tools");
    document.getElementById("version").textContent = `Version ${await invoke("app_version")}`;
  } catch (e) {
    main.replaceChildren(notice("error", `Impossible de démarrer : ${e}`));
    return;
  }
  renderNav();
  renderScale();
  render();
})();
