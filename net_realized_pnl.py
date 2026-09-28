"""Net Realized Profit / Loss hebdomadaire (style Glassnode « Profit Taking »).

Pour chaque semaine, on somme la valeur USD réalisée par les UTXO dépensés :
profit si le prix au moment de la dépense est supérieur au prix à la création
de l'UTXO, perte sinon. Le graphique superpose au prix BTC des bulles vertes
(profit net) ou rouges (perte nette) dont la taille est proportionnelle au
montant net réalisé sur la semaine.

Les données viennent d'une requête Dune Analytics (SQL de référence dans
dune_queries/net_realized_pnl.sql) dont l'ID se configure via
DUNE_QUERY_NET_REALIZED_PNL ou la section [DUNE_QUERIES] de config.ini.
"""
import numpy as np
import pandas as pd
import plotly.graph_objects as go
import streamlit as st

from config_manager import get_dune_query_id
from data_provider import get_dune_query_results, get_ticker_history

TIME_COLUMNS = ['week', 'time', 'date', 'day', 'block_time']
MIN_BUBBLE_SIZE = 6
MAX_BUBBLE_SIZE = 40


def _find_column(df, predicate):
    return next((c for c in df.columns if predicate(c.lower())), None)


def compute_weekly_net_realized(df):
    """Normalise les lignes Dune en un DataFrame hebdomadaire.

    Accepte soit des colonnes profit/perte séparées (la perte pouvant être
    signée négativement ou positivement), soit une colonne nette unique.
    Retourne les colonnes ['week', 'profit', 'loss', 'net'] (perte en valeur
    absolue, net = profit - perte), agrégées par semaine commençant le lundi.
    """
    time_col = next((c for c in TIME_COLUMNS if c in df.columns), None)
    if time_col is None:
        raise ValueError(f"Aucune colonne temporelle trouvée. Colonnes dispos : {list(df.columns)}")

    profit_col = _find_column(df, lambda c: 'profit' in c and 'loss' not in c and 'net' not in c)
    loss_col = _find_column(df, lambda c: 'loss' in c and 'profit' not in c and 'net' not in c)
    net_col = _find_column(df, lambda c: 'net' in c)

    out = pd.DataFrame({'time': pd.to_datetime(df[time_col]).dt.tz_localize(None)})
    if profit_col and loss_col:
        out['profit'] = pd.to_numeric(df[profit_col], errors='coerce').fillna(0)
        out['loss'] = pd.to_numeric(df[loss_col], errors='coerce').fillna(0).abs()
    elif net_col:
        net = pd.to_numeric(df[net_col], errors='coerce').fillna(0)
        out['profit'] = net.clip(lower=0)
        out['loss'] = (-net).clip(lower=0)
    else:
        raise ValueError(
            f"Colonnes profit/perte introuvables. Colonnes dispos : {list(df.columns)}"
        )

    out['week'] = out['time'].dt.to_period('W-SUN').dt.start_time
    weekly = out.groupby('week', as_index=False)[['profit', 'loss']].sum()
    weekly['net'] = weekly['profit'] - weekly['loss']
    return weekly.sort_values('week').reset_index(drop=True)


def bubble_sizes(values, min_size=MIN_BUBBLE_SIZE, max_size=MAX_BUBBLE_SIZE):
    """Taille de bulle proportionnelle à la racine de |valeur| (aire ∝ montant)."""
    magnitude = np.sqrt(np.abs(np.asarray(values, dtype=float)))
    peak = magnitude.max() if magnitude.size else 0
    if peak == 0:
        return np.full(magnitude.shape, float(min_size))
    return min_size + (max_size - min_size) * magnitude / peak


def _format_usd(value):
    abs_value = abs(value)
    for threshold, suffix in ((1e9, 'Md'), (1e6, 'M'), (1e3, 'k')):
        if abs_value >= threshold:
            return f"{value / threshold:,.2f} {suffix}$"
    return f"{value:,.0f} $"


def build_net_realized_figure(weekly, price):
    """Construit le graphique prix + bulles à partir des données hebdomadaires.

    `price` est une Series de clôtures journalières indexée par date.
    """
    weekly_close = price.resample('W-SUN').last()
    weekly_close.index = weekly_close.index.to_period('W-SUN').start_time
    weekly = weekly.merge(
        weekly_close.rename('price'), left_on='week', right_index=True, how='left'
    ).dropna(subset=['price'])

    sizes = bubble_sizes(weekly['net'])
    weekly = weekly.assign(size=sizes)
    hover = weekly.apply(
        lambda r: (
            f"Semaine du {r['week']:%d/%m/%Y}<br>Prix : {r['price']:,.0f} $"
            f"<br>Profit réalisé : {_format_usd(r['profit'])}"
            f"<br>Perte réalisée : {_format_usd(r['loss'])}"
            f"<br><b>Net : {_format_usd(r['net'])}</b>"
        ),
        axis=1,
    )
    weekly = weekly.assign(hover=hover)

    fig = go.Figure()
    fig.add_trace(go.Scatter(
        x=price.index, y=price.values, mode='lines', name='Prix BTC',
        line=dict(color='rgba(255, 255, 255, 0.85)', width=1.5),
        hovertemplate='%{x|%d/%m/%Y}<br>%{y:,.0f} $<extra></extra>',
    ))

    for label, mask, color in (
        ('Net realized profit (semaine)', weekly['net'] >= 0, 'rgba(46, 204, 113, 0.55)'),
        ('Net realized loss (semaine)', weekly['net'] < 0, 'rgba(231, 76, 60, 0.55)'),
    ):
        subset = weekly[mask]
        fig.add_trace(go.Scatter(
            x=subset['week'], y=subset['price'], mode='markers', name=label,
            marker=dict(size=subset['size'], color=color, line=dict(width=1, color=color.replace('0.55', '1'))),
            text=subset['hover'], hovertemplate='%{text}<extra></extra>',
        ))

    fig.update_layout(
        title="Bitcoin : Net Realized Profit / Loss hebdomadaire",
        template="plotly_dark",
        height=700,
        hovermode="closest",
        yaxis=dict(title="Prix BTC (USD)"),
        xaxis=dict(title="Date"),
        legend=dict(orientation="h", y=1.05, x=0),
    )
    return fig


@st.cache_data(ttl=3600)
def get_net_realized_pnl_plot():
    query_id = get_dune_query_id('net_realized_pnl', '')
    if not query_id:
        st.warning(
            "Aucune requête Dune configurée pour cet indicateur. Créez une requête sur dune.com "
            "avec le SQL de `dune_queries/net_realized_pnl.sql`, puis renseignez son ID dans "
            "config.ini (section `[DUNE_QUERIES]`, clé `net_realized_pnl`) ou via la variable "
            "d'environnement `DUNE_QUERY_NET_REALIZED_PNL`."
        )
        return None

    df, error = get_dune_query_results(query_id, timeout=60)
    if error:
        st.error(error)
        return None
    if df is None or df.empty:
        st.warning("Aucune donnée retournée par la requête Dune.")
        return None

    try:
        weekly = compute_weekly_net_realized(df)
    except ValueError as e:
        st.error(str(e))
        return None

    btc = get_ticker_history('BTC-USD', start=weekly['week'].min().strftime('%Y-%m-%d'))
    if btc.empty:
        st.error("Impossible de récupérer les prix BTC via yfinance.")
        return None
    price = btc['Close'].copy()
    price.index = pd.to_datetime(price.index).tz_localize(None)

    return build_net_realized_figure(weekly, price)
