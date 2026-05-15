import pandas as pd
import numpy as np
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import streamlit as st
import requests
from io import StringIO
import yfinance as yf
from datetime import datetime

@st.cache_data(ttl=86400)
def fetch_combined_btc_data():
    # ====================== 1. Données anciennes (2010-2018) ======================
    url = "https://raw.githubusercontent.com/Yrzxiong/Bitcoin-Dataset/master/bitcoin_dataset.csv"
    try:
        response = requests.get(url, timeout=10)
        btc_old = pd.read_csv(StringIO(response.text))
        btc_old['date'] = pd.to_datetime(btc_old['Date'])
        btc_old['close'] = pd.to_numeric(btc_old['btc_market_price'], errors='coerce')
        btc_old = btc_old[['date', 'close']].dropna().sort_values('date').reset_index(drop=True)
    except Exception as e:
        st.error(f"Erreur lors du téléchargement des données historiques : {e}")
        btc_old = pd.DataFrame(columns=['date', 'close'])

    # ====================== 2. Données récentes ======================
    btc_new = yf.download('BTC-USD', start='2018-01-01', interval='1d', progress=False)
    if not btc_new.empty:
        if isinstance(btc_new.columns, pd.MultiIndex):
            btc_new = pd.DataFrame({'close': btc_new['Close']['BTC-USD']})
        else:
            btc_new = btc_new[['Close']]
            btc_new.columns = ['close']
        btc_new = btc_new.reset_index()
        btc_new.columns = ['date', 'close']
    else:
        btc_new = pd.DataFrame(columns=['date', 'close'])

    # ====================== 3. Fusion ======================
    btc = pd.concat([btc_old, btc_new], ignore_index=True)
    btc = btc.drop_duplicates(subset='date').sort_values('date').reset_index(drop=True)

    # Filtrer les prix <= 0 pour éviter les problèmes d'échelle logarithmique
    btc = btc[btc['close'] > 0].reset_index(drop=True)

    return btc

def find_corrections(df, min_drop=-15):
    corrections = []
    if df.empty:
        return corrections
    peak = df['close'].iloc[0]
    for i in range(1, len(df)):
        if df['close'].iloc[i] > peak:
            peak = df['close'].iloc[i]
        else:
            drop = (df['close'].iloc[i] / peak - 1) * 100
            if drop <= min_drop:
                corrections.append(drop)
    return corrections

def get_bmi_plot():
    btc = fetch_combined_btc_data()
    if btc.empty:
        return None

    # ====================== Cycles ======================
    cycles = [
        {"cycle": "Cycle 2011-2013", "bottom": "2011-11-01", "top": "2013-12-17"},
        {"cycle": "Cycle 2013-2017", "bottom": "2015-01-15", "top": "2017-12-17"},
        {"cycle": "Cycle 2018-2021", "bottom": "2018-12-15", "top": "2021-11-10"},
        {"cycle": "Cycle 2022-2025", "bottom": "2022-11-21", "top": "2025-10-15"}
    ]

    results = []
    for c in cycles:
        bottom_date = pd.to_datetime(c["bottom"])
        top_date    = pd.to_datetime(c["top"])

        period = btc[(btc['date'] >= bottom_date) & (btc['date'] <= top_date)].copy()
        corrections = find_corrections(period, min_drop=-15)

        if corrections:
            avg_corr = np.abs(corrections).mean()
            max_corr = np.abs(corrections).max()
            results.append({
                "cycle": c["cycle"],
                "bottom": c["bottom"],
                "top": c["top"],
                "avg_correction_%": round(avg_corr, 1),
                "max_correction_%": round(max_corr, 1),
                "nb_corrections": len(corrections)
            })

    df_results = pd.DataFrame(results)

    # ====================== Graphique ======================
    fig = make_subplots(
        rows=2, cols=1,
        shared_xaxes=True,
        vertical_spacing=0.12,
        row_heights=[0.65, 0.35],
        subplot_titles=("Prix BTC (échelle logarithmique)", "Comparaison des corrections par cycle")
    )

    # Prix BTC
    fig.add_trace(go.Scatter(
        x=btc['date'], y=btc['close'],
        mode='lines', name='Prix BTC',
        line=dict(color='#00CCFF', width=2)
    ), row=1, col=1)

    # Halvings
    halvings = {
        '2012-11-28': '1er Halving',
        '2016-07-09': '2ème Halving',
        '2020-05-11': '3ème Halving',
        '2024-04-20': '4ème Halving'
    }

    for date_str, label in halvings.items():
        dt = pd.to_datetime(date_str)
        if dt >= btc['date'].min():
            fig.add_vline(x=dt, line=dict(color="red", width=2, dash="dash"), row=1, col=1)
            fig.add_annotation(
                x=dt, y=btc['close'].max()*0.9,
                text=label, showarrow=True, arrowhead=2,
                arrowcolor="red", font=dict(color="white", size=10),
                bgcolor="rgba(200,0,0,0.8)", row=1, col=1
            )

    # Tops et Bottoms des cycles
    for c in cycles:
        bottom = pd.to_datetime(c["bottom"])
        top = pd.to_datetime(c["top"])

        if bottom >= btc['date'].min():
            fig.add_vline(x=bottom, line=dict(color="lime", width=2, dash="dot"), row=1, col=1)
            fig.add_annotation(x=bottom, y=btc['close'].max()*0.75, text="Bottom",
                               showarrow=True, arrowcolor="lime", font=dict(size=9), row=1, col=1)

        if top <= btc['date'].max():
            fig.add_vline(x=top, line=dict(color="orange", width=2, dash="dot"), row=1, col=1)
            fig.add_annotation(x=top, y=btc['close'].max()*0.95, text="Top",
                               showarrow=True, arrowcolor="orange", font=dict(size=9), row=1, col=1)

    # Barres des corrections
    fig.add_trace(go.Bar(
        x=df_results['cycle'],
        y=df_results['avg_correction_%'],
        name='Moyenne des corrections',
        marker_color='#FFAA00',
        hovertemplate="Cycle: %{x}<br>Moyenne: %{y:.1f}%<extra></extra>"
    ), row=2, col=1)

    fig.add_trace(go.Bar(
        x=df_results['cycle'],
        y=df_results['max_correction_%'],
        name='Correction max',
        marker_color='#FF3333',
        hovertemplate="Cycle: %{x}<br>Max: %{y:.1f}%<extra></extra>"
    ), row=2, col=1)

    fig.update_layout(
        xaxis_title="Date",
        yaxis_title="BTC Price (USD)",
        yaxis2_title="Correction (%)",
        template="plotly_dark",
        height=850,
        hovermode="x unified",
        barmode='group',
        margin=dict(t=100)
    )

    fig.update_yaxes(type="log", row=1, col=1)

    return fig
