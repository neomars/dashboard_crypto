import pandas as pd
import requests
import plotly.graph_objects as go
import streamlit as st
import warnings

from data_provider import get_ticker_history

warnings.filterwarnings("ignore")

REQUEST_TIMEOUT = 15

@st.cache_data(ttl=3600)
def get_btc_fear_greed_plot():
    # Fear & Greed
    url = "https://api.alternative.me/fng/?limit=0"
    try:
        response = requests.get(url, timeout=REQUEST_TIMEOUT)
        response.raise_for_status()
        payload = response.json()
    except Exception:
        return None

    if 'data' not in payload:
        return None

    fg = pd.DataFrame(payload['data'])
    fg['timestamp'] = pd.to_numeric(fg['timestamp'], errors='coerce')
    fg = fg.dropna(subset=['timestamp'])
    fg['timestamp'] = pd.to_datetime(fg['timestamp'], unit='s')
    fg = fg.rename(columns={'value': 'fear_greed'})
    fg['fear_greed'] = fg['fear_greed'].astype(int)
    fg = fg[['timestamp', 'fear_greed']].sort_values('timestamp').reset_index(drop=True)

    # BTC
    btc = get_ticker_history('BTC-USD', start='2010-01-01')
    if btc.empty:
        return None
    btc = btc[['Close']].reset_index()
    btc.columns = ['timestamp', 'close']

    # Merge
    df = pd.merge(btc, fg, on='timestamp', how='left')
    df['fear_greed'] = df['fear_greed'].ffill().fillna(50)

    def get_color(fg_value):
        r = int(255 * (1 - fg_value / 100))
        g = int(255 * (fg_value / 100))
        return f"rgb({r},{g},0)"

    # Graphique
    fig = go.Figure()

    # Regroupe les jours consécutifs par tranche de 5 points de F&G : une trace
    # Plotly par jour (des milliers sur l'historique complet) ralentissait fortement le rendu.
    df['color_bucket'] = (df['fear_greed'] // 5) * 5
    segment_starts = df.index[df['color_bucket'].ne(df['color_bucket'].shift())].tolist()
    segment_bounds = segment_starts + [len(df)]

    for start, end in zip(segment_bounds[:-1], segment_bounds[1:]):
        seg_end = min(end + 1, len(df))  # +1 pour relier visuellement les segments entre eux
        color = get_color(df['color_bucket'].iloc[start])
        fig.add_trace(go.Scatter(
            x=df['timestamp'].iloc[start:seg_end],
            y=df['close'].iloc[start:seg_end],
            mode='lines',
            line=dict(color=color, width=3),
            hoverinfo='skip'
        ))

    # Trace pour le hover
    fig.add_trace(go.Scatter(
        x=df['timestamp'],
        y=df['close'],
        mode='markers',
        marker=dict(size=0.1, color='rgba(0,0,0,0)'),
        customdata=df['fear_greed'],
        hovertemplate=
            "<b>%{x|%Y-%m-%d}</b><br>" +
            "BTC: $%{y:,.0f}<br>" +
            "Fear & Greed: %{customdata:.0f}/100<br>" +
            "<extra></extra>"
    ))

    fig.update_layout(
        title="BTC Price (depuis 2010) — Coloré par Fear & Greed Index",
        xaxis_title="Date",
        yaxis_title="BTC Price (USD)",
        template="plotly_dark",
        hovermode="x unified",
        height=720,
        showlegend=False
    )

    return fig

if __name__ == "__main__":
    # If run standalone, st.cache_data won't work as expected without a streamlit context
    # but it usually just falls back to running the function.
    print("Récupération des données BTC + Fear & Greed depuis 2018...")
    fig = get_btc_fear_greed_plot()
    if fig:
        fig.show()
        print("✅ Graphique ouvert ! Courbe BTC colorée selon le Fear & Greed (Daily uniquement).")
    else:
        print("❌ Erreur lors de la récupération des données.")
