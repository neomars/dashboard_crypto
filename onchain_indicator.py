import pandas as pd
import yfinance as yf
import plotly.graph_objects as go
import streamlit as st
import warnings

warnings.filterwarnings("ignore")

@st.cache_data(ttl=3600)
def get_onchain_plot():
    # ==================== Données BTC ====================
    # On utilise 2010 pour avoir le maximum de données comme demandé précédemment
    btc = yf.download('BTC-USD', start='2010-01-01', interval='1d', progress=False)
    btc = btc[['Close']].reset_index()
    btc.columns = ['timestamp', 'close']

    price = btc['close']

    # 1. 200-week SMA (rouge) - 1400 jours approx
    sma200w = price.rolling(window=1400).mean()

    # 2. Realized Price approximation (orange) - 730 jours
    realized = price.rolling(window=730).mean()

    # 3. Pi Cycle Top - 350d SMA × 2 (vert/jaune)
    sma350 = price.rolling(window=350).mean()
    pi_cycle = sma350 * 2

    # 4. 111-day SMA × 2 (bleu clair)
    sma111 = price.rolling(window=111).mean()
    pi_cycle2 = sma111 * 2

    # 5. 2-year SMA (bleu foncé)
    sma2y = price.rolling(window=730).mean()

    # ==================== Graphique ====================
    fig = go.Figure()

    # BTC Price (ligne blanche épaisse)
    fig.add_trace(go.Scatter(x=btc['timestamp'], y=price,
                             mode='lines', name='BTC Price',
                             line=dict(color='white', width=2)))

    # Indicateurs
    fig.add_trace(go.Scatter(x=btc['timestamp'], y=sma200w,
                             mode='lines', name='200-week SMA',
                             line=dict(color='#FF3333', width=1.5)))

    fig.add_trace(go.Scatter(x=btc['timestamp'], y=realized,
                             mode='lines', name='Realized Price (730d SMA)',
                             line=dict(color='#FFAA33', width=1.5)))

    fig.add_trace(go.Scatter(x=btc['timestamp'], y=pi_cycle,
                             mode='lines', name='Pi Cycle Top (350d × 2)',
                             line=dict(color='#33FF99', width=1.5)))

    fig.add_trace(go.Scatter(x=btc['timestamp'], y=pi_cycle2,
                             mode='lines', name='111d SMA × 2',
                             line=dict(color='#33AAFF', width=1.5)))

    fig.add_trace(go.Scatter(x=btc['timestamp'], y=sma2y,
                             mode='lines', name='2-year SMA',
                             line=dict(color='#3366FF', width=1.5, dash='dot')))

    fig.update_layout(
        title="On-chain Top & Bottom Indicators",
        xaxis_title="Date",
        yaxis_title="BTC Price (USD)",
        template="plotly_dark",
        height=800,
        legend=dict(x=0.01, y=0.99, bgcolor="rgba(0,0,0,0.6)"),
        hovermode="x unified"
    )

    return fig

if __name__ == "__main__":
    print("Récupération des données BTC + Indicateurs On-chain...")
    fig = get_onchain_plot()
    if fig:
        fig.show()
        print("✅ Graphique ouvert !")
    else:
        print("❌ Erreur lors de la récupération des données.")
