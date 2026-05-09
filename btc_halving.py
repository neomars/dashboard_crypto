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
    # ====================== Halvings connus ======================
    known_halvings = [
        {"date": "2012-11-28", "block": 210000,  "reward": "50 → 25"},
        {"date": "2016-07-09", "block": 420000,  "reward": "25 → 12.5"},
        {"date": "2020-05-11", "block": 630000,  "reward": "12.5 → 6.25"},
        {"date": "2024-04-20", "block": 840000,  "reward": "6.25 → 3.125"}
    ]
    halving_dates = [pd.to_datetime(h["date"]) for h in known_halvings]

    # ====================== Prochain halving ======================
    try:
        current_block = int(requests.get("https://mempool.space/api/blocks/tip/height", timeout=10).text.strip())
    except:
        current_block = 840000

    block_interval = 210000
    next_halving_block = ((current_block // block_interval) + 1) * block_interval
    blocks_to_next = next_halving_block - current_block
    estimated_date = datetime.now() + timedelta(days=(blocks_to_next * 10) / (60 * 24))

    all_halvings = halving_dates + [pd.to_datetime(estimated_date)]

    # ====================== BTC Price ======================
    btc = yf.download('BTC-USD', start='2010-01-01', interval='1d', progress=False)
    btc = btc[['Close']].reset_index()
    btc.columns = ['timestamp', 'close']

    # ====================== Calculs Jours ======================
    def get_days_info(ts):
        past_halvings = [h for h in all_halvings if h <= ts]
        days_after = (ts - past_halvings[-1]).days if past_halvings else 0

        future_halvings = [h for h in all_halvings if h > ts]
        days_before = (future_halvings[0] - ts).days if future_halvings else 0

        return days_after, days_before

    btc['days_after'], btc['days_before'] = zip(*btc['timestamp'].map(get_days_info))

    # ====================== Identification des Tops ======================
    tops = []
    for i in range(len(all_halvings) - 1):
        start_date = all_halvings[i]
        end_date = all_halvings[i+1]

        period_data = btc[(btc['timestamp'] >= start_date) & (btc['timestamp'] < end_date)]
        if not period_data.empty:
            top_row = period_data.loc[period_data['close'].idxmax()]
            tops.append(top_row)

    # ====================== Graphique ======================
    fig = go.Figure()

    # Courbe BTC
    fig.add_trace(go.Scatter(
        x=btc['timestamp'],
        y=btc['close'],
        mode='lines',
        name='BTC Price',
        line=dict(color='#FF9900', width=2),
        customdata=btc[['days_after', 'days_before']],
        hovertemplate=
            "<b>Date: %{x|%d %b %Y}</b><br>" +
            "Prix: $%{y:,.0f}<br>" +
            "Jours après halving: %{customdata[0]}<br>" +
            "Jours avant prochain: %{customdata[1]}<br>" +
            "<extra></extra>"
    ))

    # Halvings historiques
    for h in known_halvings:
        date = pd.to_datetime(h["date"])
        if date >= btc['timestamp'].min():
            fig.add_vline(x=date, line=dict(color="red", width=2, dash="dash"))
            fig.add_annotation(
                x=date, y=1, yref="paper", yanchor="top",
                text=f"Halving {h['date']}",
                showarrow=False,
                font=dict(color="red") # This 'font' is for Annotation, which IS valid.
            )

    # Tops
    for top in tops:
        fig.add_trace(go.Scatter(
            x=[top['timestamp']],
            y=[top['close']],
            mode='markers+text',
            name='Cycle Top',
            text=[f"Top: {top['days_after']}j après"],
            textposition="top center",
            marker=dict(color='white', size=14, symbol='star', line=dict(color='black', width=1)),
            showlegend=False
        ))

    # Prochain halving
    fig.add_vline(x=estimated_date, line=dict(color="lime", width=3, dash="dot"))
    fig.add_annotation(
        x=estimated_date, y=0.9, yref="paper", yanchor="top",
        text=f"Prochain Halving<br>~{estimated_date.strftime('%Y-%m-%d')}",
        bgcolor="lime",
        font=dict(color="black") # This 'font' is for Annotation, which IS valid.
    )

    fig.update_layout(
        title="BTC Price, Halvings & Tops de Cycles",
        xaxis_title="Date",
        yaxis_title="BTC Price (USD)",
        template="plotly_dark",
        height=800,
        hovermode="x unified",
        yaxis_type="log"
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
