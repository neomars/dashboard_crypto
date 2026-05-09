import pandas as pd
import yfinance as yf
import requests
import plotly.graph_objects as go
from datetime import datetime, timedelta
import streamlit as st
import warnings

warnings.filterwarnings("ignore")

@st.cache_data(ttl=3600)
def get_btc_halving_plot():
    # ====================== Halvings connus (source : blockchain publique) ======================
    known_halvings = [
        {"date": "2012-11-28", "block": 210000,  "reward": "50 → 25"},
        {"date": "2016-07-09", "block": 420000,  "reward": "25 → 12.5"},
        {"date": "2020-05-11", "block": 630000,  "reward": "12.5 → 6.25"},
        {"date": "2024-04-20", "block": 840000,  "reward": "6.25 → 3.125"}
    ]

    # ====================== Récupération du block height actuel (API ouverte) ======================
    try:
        current_block = int(requests.get("https://mempool.space/api/blocks/tip/height", timeout=10).text.strip())
    except:
        current_block = 840000 # Fallback near last halving if API fails

    # ====================== Calcul du prochain halving ======================
    block_interval = 210000
    next_halving_block = ((current_block // block_interval) + 1) * block_interval
    blocks_to_next = next_halving_block - current_block

    # Estimation de la date (10 minutes par bloc en moyenne)
    minutes_to_next = blocks_to_next * 10
    days_to_next = minutes_to_next / (60 * 24)
    estimated_date = datetime.now() + timedelta(days=days_to_next)

    # ====================== BTC Price ======================
    btc = yf.download('BTC-USD', start='2018-01-01', interval='1d', progress=False)
    btc = btc[['Close']].reset_index()
    btc.columns = ['timestamp', 'close']

    # ====================== Graphique ======================
    fig = go.Figure()

    # Courbe BTC
    fig.add_trace(go.Scatter(
        x=btc['timestamp'],
        y=btc['close'],
        mode='lines',
        name='BTC Price',
        line=dict(color='#FF9900', width=2)
    ))

    # Max price for annotations placement
    max_price = btc['close'].max()

    # Halvings historiques
    for h in known_halvings:
        date = pd.to_datetime(h["date"])
        # Only show if within our price data range (post 2018)
        if date >= btc['timestamp'].min():
            fig.add_vline(x=date, line=dict(color="red", width=2, dash="dash"))
            fig.add_annotation(
                x=date,
                y=max_price * 0.92,
                text=f"Halving {h['date']}<br>{h['reward']}",
                showarrow=True,
                arrowhead=2,
                arrowcolor="red",
                font=dict(color="white", size=11),
                bgcolor="rgba(200,0,0,0.8)"
            )

    # Prochain halving (dynamique)
    fig.add_vline(x=estimated_date, line=dict(color="lime", width=3, dash="dot"))
    fig.add_annotation(
        x=estimated_date,
        y=max_price * 0.85,
        text=f"5ème Halving<br>~ {estimated_date.strftime('%Y-%m-%d')}<br>dans {days_to_next:.0f} jours",
        showarrow=True,
        arrowhead=2,
        arrowcolor="lime",
        font=dict(color="black", size=12),
        bgcolor="lime"
    )

    fig.update_layout(
        title=f"BTC Price + Halvings<br>"
              f"Prochain halving estimé : {estimated_date.strftime('%d %B %Y')} ({days_to_next:.0f} jours)",
        xaxis_title="Date",
        yaxis_title="BTC Price (USD)",
        template="plotly_dark",
        height=780,
        hovermode="x unified"
    )

    return fig

if __name__ == "__main__":
    print("Récupération des données BTC + Halvings...")
    fig = get_btc_halving_plot()
    if fig:
        fig.show()
        print("✅ Graphique ouvert !")
    else:
        print("❌ Erreur lors de la récupération des données.")
