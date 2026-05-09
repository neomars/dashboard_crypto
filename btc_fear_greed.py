import pandas as pd
import yfinance as yf
import requests
import plotly.graph_objects as go
import streamlit as st
import warnings

warnings.filterwarnings("ignore")

@st.cache_data(ttl=3600)
def get_btc_fear_greed_plot():
    # Fear & Greed
    url = "https://api.alternative.me/fng/?limit=0"
    try:
        response = requests.get(url).json()
    except Exception:
        return None

    if 'data' not in response:
        return None

    fg = pd.DataFrame(response['data'])
    fg['timestamp'] = pd.to_numeric(fg['timestamp'], errors='coerce')
    fg = fg.dropna(subset=['timestamp'])
    fg['timestamp'] = pd.to_datetime(fg['timestamp'], unit='s')
    fg = fg.rename(columns={'value': 'fear_greed'})
    fg['fear_greed'] = fg['fear_greed'].astype(int)
    fg = fg[['timestamp', 'fear_greed']].sort_values('timestamp').reset_index(drop=True)

    # BTC
    btc = yf.download('BTC-USD', start='2010-01-01', interval='1d', progress=False)
    btc = btc[['Close']].reset_index()
    btc.columns = ['timestamp', 'close']

    # Merge
    df = pd.merge(btc, fg, on='timestamp', how='left')
    df['fear_greed'] = df['fear_greed'].ffill().fillna(50)

    def get_color(fg_value):
        r = int(255 * (1 - fg_value / 100))
        g = int(255 * (fg_value / 100))
        return f"rgb({r},{g},0)"

    # Graphique simple (seulement Daily)
    fig = go.Figure()

    # Segments colorés
    # Note: Adding many traces can be slow in Plotly.
    # To optimize, we could group consecutive segments with similar F&G values,
    # but for now let's stick to the user's logic while allowing it to be cached.
    for i in range(len(df) - 1):
        color = get_color(df['fear_greed'].iloc[i])
        fig.add_trace(go.Scatter(
            x=df['timestamp'].iloc[i:i+2],
            y=df['close'].iloc[i:i+2],
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
